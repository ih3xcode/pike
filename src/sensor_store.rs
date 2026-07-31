use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;
use tokio::time::Instant;

use crate::error::AppError;
use crate::falcon_api::SensorMeta;

/// Джерело списку доступних сенсорів.
/// `+ Send` на майбутньому обовʼязковий: axum вимагає Send-хендлери.
pub trait SensorLister: Send + Sync + 'static {
    fn list(&self, platform: &str)
    -> impl Future<Output = Result<Vec<SensorMeta>, AppError>> + Send;
}

/// Джерело байтів сенсора за його sha256.
pub trait SensorDownloader: Send + Sync + 'static {
    fn fetch(&self, sha256: &str) -> impl Future<Output = Result<bytes::Bytes, AppError>> + Send;
}

/// Мінімальна пауза між спробами після невдачі. Без неї при недоступному
/// API кожен запит /cb платив би повний мережевий таймаут.
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60);

struct Snapshot {
    metas: Vec<SensorMeta>,
    fetched_at: Instant,
}

#[derive(Default)]
struct CacheState {
    snapshots: HashMap<String, Snapshot>,
    /// Коли востаннє провалився запит по платформі. Живе окремо від знімків
    /// навмисно: на холодному старті знімка ще немає, а саме тоді пауза між
    /// спробами й потрібна найбільше.
    failures: HashMap<String, Instant>,
}

/// Список сенсорів з API зі скінченним часом життя.
///
/// Свіжість тут — головний захист від застигання версій: матчинг завжди
/// відбувається по актуальному списку, а не по тому, що колись потрапило в кеш.
pub struct MetadataCache<L: SensorLister> {
    lister: Arc<L>,
    ttl: Duration,
    state: tokio::sync::Mutex<CacheState>,
}

impl<L: SensorLister> MetadataCache<L> {
    pub fn new(lister: Arc<L>, ttl: Duration) -> Self {
        Self {
            lister,
            ttl,
            state: tokio::sync::Mutex::new(CacheState::default()),
        }
    }

    pub async fn get(&self, platform: &str) -> Result<Vec<SensorMeta>, AppError> {
        // Лок тримається через await навмисно: це серіалізує паралельні
        // оновлення одного платформного списку в один запит до API.
        let mut guard = self.state.lock().await;
        let now = Instant::now();

        if let Some(snapshot) = guard.snapshots.get(platform) {
            if now.duration_since(snapshot.fetched_at) < self.ttl {
                return Ok(snapshot.metas.clone());
            }
        }

        // Недавня невдача — не б'ємо по API повторно незалежно від того,
        // чи є що віддати з кешу
        if let Some(failed_at) = guard.failures.get(platform) {
            if now.duration_since(*failed_at) < RETRY_AFTER_FAILURE {
                return match guard.snapshots.get(platform) {
                    Some(snapshot) => Ok(snapshot.metas.clone()),
                    None => Err(AppError::Other(format!(
                        "sensor list for '{platform}' unavailable; \
                         the last API call failed and the retry window has not elapsed"
                    ))),
                };
            }
        }

        match self.lister.list(platform).await {
            Ok(metas) => {
                let result = metas.clone();
                guard.failures.remove(platform);
                guard.snapshots.insert(
                    platform.to_string(),
                    Snapshot {
                        metas,
                        fetched_at: Instant::now(),
                    },
                );
                Ok(result)
            }
            Err(e) => {
                guard.failures.insert(platform.to_string(), Instant::now());
                match guard.snapshots.get(platform) {
                    Some(snapshot) => {
                        eprintln!(
                            "[meta] WARNING: refresh for '{platform}' failed ({e}); \
                             serving snapshot from cache"
                        );
                        Ok(snapshot.metas.clone())
                    }
                    None => Err(e),
                }
            }
        }
    }
}

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
pub struct BinaryStore<D: SensorDownloader> {
    downloader: Arc<D>,
    dir: PathBuf,
    max_bytes: u64,
    inflight: std::sync::Mutex<HashMap<String, Arc<OnceCell<()>>>>,
}

impl<D: SensorDownloader> BinaryStore<D> {
    pub fn new(downloader: Arc<D>, dir: PathBuf, max_bytes: u64) -> Self {
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
            eprintln!("[cache] Removed {removed} unfinished download(s) from {}", tmp_dir.display());
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
        let candidates: Vec<CacheEntry> =
            entries.into_iter().filter(|e| e.path != keep).collect();
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn meta(name: &str) -> SensorMeta {
        SensorMeta {
            name: name.to_string(),
            sha256: "a".repeat(64),
            platform: "linux".into(),
            os: "Ubuntu".into(),
            file_type: "deb".into(),
            file_size: 10,
            version: "7.0".into(),
            architectures: vec![],
        }
    }

    struct FakeLister {
        calls: AtomicUsize,
        fail_after: usize,
    }

    impl FakeLister {
        fn new(fail_after: usize) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                fail_after,
            })
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SensorLister for FakeLister {
        async fn list(&self, _platform: &str) -> Result<Vec<SensorMeta>, AppError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n >= self.fail_after {
                Err(AppError::Other("api down".into()))
            } else {
                Ok(vec![meta(&format!("sensor{n}.deb"))])
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn fetches_once_then_serves_from_cache() {
        let lister = FakeLister::new(usize::MAX);
        let cache = MetadataCache::new(lister.clone(), Duration::from_secs(3600));

        let a = cache.get("linux").await.unwrap();
        let b = cache.get("linux").await.unwrap();

        assert_eq!(lister.calls(), 1);
        assert_eq!(a[0].name, b[0].name);
    }

    #[tokio::test(start_paused = true)]
    async fn refetches_after_ttl() {
        let lister = FakeLister::new(usize::MAX);
        let cache = MetadataCache::new(lister.clone(), Duration::from_secs(60));

        cache.get("linux").await.unwrap();
        tokio::time::advance(Duration::from_secs(61)).await;
        let second = cache.get("linux").await.unwrap();

        assert_eq!(lister.calls(), 2);
        assert_eq!(second[0].name, "sensor1.deb");
    }

    #[tokio::test(start_paused = true)]
    async fn serves_stale_snapshot_when_api_fails() {
        let lister = FakeLister::new(1); // перший виклик успішний, далі помилки
        let cache = MetadataCache::new(lister.clone(), Duration::from_secs(60));

        cache.get("linux").await.unwrap();
        tokio::time::advance(Duration::from_secs(61)).await;
        let stale = cache.get("linux").await.unwrap();

        assert_eq!(stale[0].name, "sensor0.deb");
        assert_eq!(lister.calls(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn negative_caching_suppresses_immediate_retry() {
        let lister = FakeLister::new(1);
        let cache = MetadataCache::new(lister.clone(), Duration::from_secs(60));

        cache.get("linux").await.unwrap();
        tokio::time::advance(Duration::from_secs(61)).await;
        cache.get("linux").await.unwrap(); // невдача, віддає застаріле
        cache.get("linux").await.unwrap(); // не має бити по API знову

        assert_eq!(lister.calls(), 2);

        tokio::time::advance(Duration::from_secs(61)).await;
        cache.get("linux").await.unwrap();
        assert_eq!(lister.calls(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn negative_caching_applies_without_any_snapshot() {
        // Холодний старт із недоступним API: без паузи кожен запит /cb
        // платив би повний мережевий таймаут
        let lister = FakeLister::new(0);
        let cache = MetadataCache::new(lister.clone(), Duration::from_secs(60));

        assert!(cache.get("linux").await.is_err());
        assert!(cache.get("linux").await.is_err());
        assert!(cache.get("linux").await.is_err());
        assert_eq!(lister.calls(), 1, "повтори мали впертись у паузу");

        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(cache.get("linux").await.is_err());
        assert_eq!(lister.calls(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn errors_when_no_snapshot_at_all() {
        let lister = FakeLister::new(0); // падає одразу
        let cache = MetadataCache::new(lister.clone(), Duration::from_secs(60));

        assert!(cache.get("linux").await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn platforms_are_cached_independently() {
        let lister = FakeLister::new(usize::MAX);
        let cache = MetadataCache::new(lister.clone(), Duration::from_secs(3600));

        cache.get("linux").await.unwrap();
        cache.get("windows").await.unwrap();

        assert_eq!(lister.calls(), 2);
    }

    // --- BinaryStore ---

    use std::time::{Duration as StdDuration, SystemTime};

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
        async fn fetch(&self, _sha256: &str) -> Result<bytes::Bytes, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            // Затримка, щоб паралельні виклики справді перетнулись у часі
            tokio::time::sleep(StdDuration::from_millis(50)).await;
            if self.corrupt {
                Ok(bytes::Bytes::from_static(b"not what was promised"))
            } else {
                Ok(bytes::Bytes::from(self.payload.clone()))
            }
        }
    }

    fn sha256_of(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
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
        assert_eq!(path, dir.path().join(&sha), "маршрут /s/ приймає лише нижній регістр");
        assert!(path.exists());
    }

    // --- plan_eviction (чиста функція) ---

    fn entry(name: &str, size: u64, secs_ago: u64) -> CacheEntry {
        CacheEntry {
            path: PathBuf::from(name),
            size,
            modified: SystemTime::UNIX_EPOCH + StdDuration::from_secs(1_000_000 - secs_ago),
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
        assert_eq!(
            evicted,
            vec![PathBuf::from("oldest"), PathBuf::from("middle")]
        );
    }
}
