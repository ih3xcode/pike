use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::common::error::AppError;

use super::ports::SensorDownloader;

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub path: PathBuf,
    pub size: u64,
    pub modified: std::time::SystemTime,
}

/// Які файли видалити, щоб сума розмірів влізла в ліміт.
/// Витісняються найраніше завантажені (mtime не оновлюється при віддачі).
pub fn plan_eviction(mut entries: Vec<CacheEntry>, max_bytes: u64) -> Vec<PathBuf> {
    let mut total: u64 = entries.iter().map(|e| e.size).sum();
    if total <= max_bytes {
        return Vec::new();
    }
    entries.sort_by_key(|e| e.modified);

    let mut victims = Vec::new();
    for e in entries {
        if total <= max_bytes {
            break;
        }
        total = total.saturating_sub(e.size);
        victims.push(e.path);
    }
    victims
}

/// Дисковий кеш бінарів сенсорів. Імʼя файлу дорівнює sha256 його вмісту,
/// тому каталог самоописовий і переживає рестарт без окремого індексу.
pub struct BinaryStore {
    downloader: Arc<dyn SensorDownloader>,
    dir: PathBuf,
    max_bytes: u64,
    inflight: std::sync::Mutex<HashMap<String, Arc<OnceCell<()>>>>,
}

impl BinaryStore {
    pub fn new(downloader: Arc<dyn SensorDownloader>, dir: PathBuf, max_bytes: u64) -> Self {
        Self {
            downloader,
            dir,
            max_bytes,
            inflight: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn path_for(&self, sha256: &str) -> PathBuf {
        // Імʼя завжди в нижньому регістрі: маршрут /s/{sha256} приймає лише
        // такий, а метадані з API можуть прийти в будь-якому
        self.dir.join(sha256.to_ascii_lowercase())
    }

    /// Прибирає недокачані файли з `tmp/`. Викликається на старті, коли
    /// власних завантажень у польоті ще немає: після SIGKILL там лишаються
    /// сирі байти, які не бачить облік розміру й не чистить ніщо інше.
    pub fn sweep_tmp(&self) {
        let tmp_dir = self.dir.join("tmp");
        let Ok(entries) = std::fs::read_dir(&tmp_dir) else {
            return;
        };
        let mut removed = 0u32;
        for item in entries.flatten() {
            if item.path().is_file() && std::fs::remove_file(item.path()).is_ok() {
                removed += 1;
            }
        }
        if removed > 0 {
            eprintln!(
                "[cache] Removed {removed} unfinished download(s) from {}",
                tmp_dir.display()
            );
        }
    }

    pub async fn ensure(&self, sha256: &str) -> Result<PathBuf, AppError> {
        let path = self.path_for(sha256);
        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(path);
        }

        // Одна комірка на sha256: перший запит качає, решта чекають на нього.
        // OnceCell не запамʼятовує помилку, тому невдача не блокує повтор.
        let cell = {
            let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            map.entry(sha256.to_string())
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };

        let result = cell
            .get_or_try_init(|| self.download_and_place(sha256))
            .await
            .map(|_| ());

        {
            let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            map.remove(sha256);
        }

        result?;
        Ok(path)
    }

    async fn download_and_place(&self, sha256: &str) -> Result<(), AppError> {
        let data = self.downloader.fetch(sha256).await?;

        let actual = hex::encode(Sha256::digest(&data));
        if !actual.eq_ignore_ascii_case(sha256) {
            return Err(AppError::Other(format!(
                "sensor integrity check failed: expected sha256 {sha256}, got {actual}"
            )));
        }

        let tmp_dir = self.dir.join("tmp");
        tokio::fs::create_dir_all(&tmp_dir)
            .await
            .map_err(|e| AppError::io("Cannot create cache tmp dir", e))?;

        let suffix: [u8; 8] = rand::random();
        let tmp_path = tmp_dir.join(format!("{sha256}.{}", hex::encode(suffix)));

        tokio::fs::write(&tmp_path, &data)
            .await
            .map_err(|e| AppError::io("Cannot write sensor to cache", e))?;

        // Атомарний у межах ФС: недокачаний файл ніколи не видно під фінальним іменем
        let final_path = self.path_for(sha256);
        tokio::fs::rename(&tmp_path, &final_path)
            .await
            .map_err(|e| AppError::io("Cannot publish sensor into cache", e))?;

        self.run_gc(&final_path).await;
        Ok(())
    }

    /// `keep` — щойно опублікований файл. Він виключений з витіснення:
    /// інакше при ліміті, меншому за розмір сенсора, GC зносив би його
    /// одразу після запису, і клієнт отримував би 404 на файл, який
    /// сервер щойно пообіцяв у відповіді на /cb.
    async fn run_gc(&self, keep: &Path) {
        let entries = match Self::scan(&self.dir).await {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[cache] WARNING: cannot scan cache dir: {e}");
                return;
            }
        };
        let in_flight = Self::tmp_bytes(&self.dir).await;
        let total: u64 = entries.iter().map(|e| e.size).sum::<u64>() + in_flight;
        let candidates: Vec<CacheEntry> = entries.into_iter().filter(|e| e.path != keep).collect();
        let kept: u64 = total - candidates.iter().map(|e| e.size).sum::<u64>();

        if total > self.max_bytes && kept > self.max_bytes {
            eprintln!(
                "[cache] WARNING: {} alone exceeds cache_max_bytes ({kept} > {}); \
                 raise the limit or the cache will churn",
                keep.display(),
                self.max_bytes
            );
        }

        // Бюджет для решти файлів — ліміт мінус те, що ми зобовʼязані лишити
        let budget = self.max_bytes.saturating_sub(kept);
        for victim in plan_eviction(candidates, budget) {
            match tokio::fs::remove_file(&victim).await {
                Ok(()) => eprintln!("[cache] Evicted {}", victim.display()),
                Err(e) => eprintln!("[cache] WARNING: cannot evict {}: {e}", victim.display()),
            }
        }
    }

    async fn scan(dir: &Path) -> std::io::Result<Vec<CacheEntry>> {
        let mut out = Vec::new();
        let mut rd = tokio::fs::read_dir(dir).await?;
        while let Some(item) = rd.next_entry().await? {
            let meta = item.metadata().await?;
            if !meta.is_file() {
                continue;
            }
            out.push(CacheEntry {
                path: item.path(),
                size: meta.len(),
                modified: meta.modified()?,
            });
        }
        Ok(out)
    }

    /// Скільки байтів займають завантаження в польоті. Вони не витісняються
    /// (їх пише інша задача), але в ліміт входять — інакше кеш стабільно
    /// переростав би `cache_max_bytes` на розмір паралельних завантажень.
    async fn tmp_bytes(dir: &Path) -> u64 {
        let tmp_dir = dir.join("tmp");
        let Ok(mut rd) = tokio::fs::read_dir(&tmp_dir).await else {
            return 0;
        };
        let mut total = 0u64;
        while let Ok(Some(item)) = rd.next_entry().await {
            if let Ok(meta) = item.metadata().await {
                if meta.is_file() {
                    total += meta.len();
                }
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::ports::BoxFuture;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    struct FakeDownloader {
        calls: AtomicUsize,
        payload: Vec<u8>,
        corrupt: bool,
    }

    impl FakeDownloader {
        fn new(payload: &[u8], corrupt: bool) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                payload: payload.to_vec(),
                corrupt,
            })
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SensorDownloader for FakeDownloader {
        fn fetch<'a>(&'a self, _sha256: &'a str) -> BoxFuture<'a, Result<bytes::Bytes, AppError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                // Затримка, щоб паралельні виклики справді перетнулись у часі
                tokio::time::sleep(Duration::from_millis(50)).await;
                if self.corrupt {
                    Ok(bytes::Bytes::from_static(b"not what was promised"))
                } else {
                    Ok(bytes::Bytes::from(self.payload.clone()))
                }
            })
        }
    }

    fn sha256_of(data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(data);
        hex::encode(h.finalize())
    }

    #[tokio::test]
    async fn downloads_once_and_writes_file() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"sensor payload";
        let sha = sha256_of(payload);
        let dl = FakeDownloader::new(payload, false);
        let store = BinaryStore::new(dl.clone(), dir.path().to_path_buf(), u64::MAX);

        let path = store.ensure(&sha).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), payload);
        assert_eq!(dl.calls(), 1);

        store.ensure(&sha).await.unwrap();
        assert_eq!(dl.calls(), 1, "другий ensure не має качати повторно");
    }

    #[tokio::test]
    async fn rejects_sha256_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let sha = sha256_of(b"expected");
        let dl = FakeDownloader::new(b"expected", true);
        let store = BinaryStore::new(dl, dir.path().to_path_buf(), u64::MAX);

        assert!(store.ensure(&sha).await.is_err());
        assert!(!dir.path().join(&sha).exists(), "битий файл не має лишитись");
        let tmp = dir.path().join("tmp");
        if tmp.exists() {
            assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0);
        }
    }

    #[tokio::test]
    async fn concurrent_ensure_downloads_once() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"shared sensor";
        let sha = sha256_of(payload);
        let dl = FakeDownloader::new(payload, false);
        let store = Arc::new(BinaryStore::new(
            dl.clone(),
            dir.path().to_path_buf(),
            u64::MAX,
        ));

        let mut handles = Vec::new();
        for _ in 0..10 {
            let store = store.clone();
            let sha = sha.clone();
            handles.push(tokio::spawn(async move { store.ensure(&sha).await }));
        }
        for h in handles {
            h.await.unwrap().unwrap();
        }

        assert_eq!(
            dl.calls(),
            1,
            "десять паралельних запитів = одне завантаження"
        );
    }

    #[tokio::test]
    async fn just_downloaded_file_survives_gc() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"a sensor larger than the limit";
        let sha = sha256_of(payload);
        let dl = FakeDownloader::new(payload, false);
        // Ліміт менший за сам сенсор — GC не має зносити те, що щойно обіцяли
        let store = BinaryStore::new(dl, dir.path().to_path_buf(), 4);

        let path = store.ensure(&sha).await.unwrap();
        assert!(path.exists(), "щойно завантажений файл витіснено");
        assert_eq!(std::fs::read(&path).unwrap(), payload);
    }

    #[tokio::test]
    async fn sweep_tmp_removes_partial_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("abc.deadbeef"), b"half a sensor").unwrap();

        let dl = FakeDownloader::new(b"x", false);
        let store = BinaryStore::new(dl, dir.path().to_path_buf(), u64::MAX);
        store.sweep_tmp();

        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn uppercase_sha256_maps_to_the_lowercase_path() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"case test";
        let sha = sha256_of(payload);
        let dl = FakeDownloader::new(payload, false);
        let store = BinaryStore::new(dl, dir.path().to_path_buf(), u64::MAX);

        let path = store.ensure(&sha.to_uppercase()).await.unwrap();
        assert_eq!(
            path,
            dir.path().join(&sha),
            "маршрут /s/ приймає лише нижній регістр"
        );
        assert!(path.exists());
    }

    // --- plan_eviction (чиста функція) ---

    fn entry(name: &str, size: u64, secs_ago: u64) -> CacheEntry {
        CacheEntry {
            path: PathBuf::from(name),
            size,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - secs_ago),
        }
    }

    #[test]
    fn eviction_noop_when_under_limit() {
        let entries = vec![entry("a", 10, 100), entry("b", 10, 50)];
        assert!(plan_eviction(entries, 100).is_empty());
    }

    #[test]
    fn eviction_removes_oldest_first() {
        let entries = vec![
            entry("newest", 40, 10),
            entry("oldest", 40, 300),
            entry("middle", 40, 100),
        ];
        let evicted = plan_eviction(entries, 100);
        assert_eq!(evicted, vec![PathBuf::from("oldest")]);
    }

    #[test]
    fn eviction_continues_until_under_limit() {
        let entries = vec![
            entry("newest", 40, 10),
            entry("oldest", 40, 300),
            entry("middle", 40, 100),
        ];
        let evicted = plan_eviction(entries, 50);
        assert_eq!(evicted, vec![PathBuf::from("oldest"), PathBuf::from("middle")]);
    }
}
