use std::collections::{HashMap, HashSet, VecDeque};
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

/// Which files to remove so the total size fits the limit.
/// The earliest downloaded go first (mtime is not touched when serving).
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

/// How many recently promised sensors to protect from eviction. The answer
/// to /cb names a sha256 the host will come back for seconds later in a
/// separate request; without protection a concurrent download could evict
/// the file first, and the host would get a 404 for what the server had
/// just promised.
const RECENT_PROMISES: usize = 16;

/// A disk cache of sensor binaries. The file name equals the sha256 of its
/// contents, so the directory is self-describing and survives a restart
/// without a separate index.
pub struct BinaryStore {
    downloader: Arc<dyn SensorDownloader>,
    dir: PathBuf,
    max_bytes: u64,
    inflight: std::sync::Mutex<HashMap<String, Arc<OnceCell<()>>>>,
    recent: std::sync::Mutex<VecDeque<PathBuf>>,
}

impl BinaryStore {
    pub fn new(downloader: Arc<dyn SensorDownloader>, dir: PathBuf, max_bytes: u64) -> Self {
        Self {
            downloader,
            dir,
            max_bytes,
            inflight: std::sync::Mutex::new(HashMap::new()),
            recent: std::sync::Mutex::new(VecDeque::new()),
        }
    }

    pub fn path_for(&self, sha256: &str) -> PathBuf {
        // The name is always lowercase: the /s/{sha256} route accepts only
        // that, while API metadata may arrive in any case
        self.dir.join(sha256.to_ascii_lowercase())
    }

    /// Clears unfinished downloads out of `tmp/`. Called at startup, when no
    /// downloads of our own are in flight: after a SIGKILL raw bytes are left
    /// there that size accounting cannot see and nothing else cleans up.
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
        // Remember before checking existence: the promise to the host is the
        // same whether the file had to be downloaded or was already there
        self.remember(&path);

        if tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(path);
        }

        // One cell per sha256: the first request downloads, the rest wait on
        // it. OnceCell does not remember an error, so a failure never blocks
        // a retry.
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

        self.release(sha256, &cell);

        result?;
        Ok(path)
    }

    /// Drops only our own cell. A plain `remove` would drop someone else's:
    /// after a failed download the remaining waiters still hold the old cell
    /// and retry through it, while the next request for the same sha would
    /// find no key and start a second, concurrent download of the same file.
    fn release(&self, sha256: &str, cell: &Arc<OnceCell<()>>) {
        let mut map = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
        if map.get(sha256).is_some_and(|current| Arc::ptr_eq(current, cell)) {
            map.remove(sha256);
        }
    }

    /// Pushes a path onto the recently-promised queue, dropping the oldest.
    fn remember(&self, path: &Path) {
        let mut recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        recent.retain(|p| p != path);
        recent.push_back(path.to_path_buf());
        while recent.len() > RECENT_PROMISES {
            recent.pop_front();
        }
    }

    fn protected(&self) -> HashSet<PathBuf> {
        let recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        recent.iter().cloned().collect()
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

        // Every error path cleans up after itself: sweep_tmp only runs at
        // startup, so in a long-lived service an abandoned fragment would
        // count against the limit forever and make the cache over-evict
        if let Err(e) = tokio::fs::write(&tmp_path, &data).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(AppError::io("Cannot write sensor to cache", e));
        }

        // Atomic within the filesystem: a partial file is never visible under the final name
        let final_path = self.path_for(sha256);
        if let Err(e) = tokio::fs::rename(&tmp_path, &final_path).await {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(AppError::io("Cannot publish sensor into cache", e));
        }

        self.run_gc().await;
        Ok(())
    }

    /// Recently promised sensors are excluded from eviction — otherwise, with
    /// a limit smaller than their combined size, the GC would remove a file
    /// right after writing it and the client would get a 404 for what the
    /// server had just promised in its answer to /cb.
    async fn run_gc(&self) {
        let entries = match Self::scan(&self.dir).await {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[cache] WARNING: cannot scan cache dir: {e}");
                return;
            }
        };
        let in_flight = Self::tmp_bytes(&self.dir).await;
        let total: u64 = entries.iter().map(|e| e.size).sum::<u64>() + in_flight;
        let protected = self.protected();
        let candidates: Vec<CacheEntry> = entries
            .into_iter()
            .filter(|e| !protected.contains(&e.path))
            .collect();
        let kept: u64 = total - candidates.iter().map(|e| e.size).sum::<u64>();

        if total > self.max_bytes && kept > self.max_bytes {
            eprintln!(
                "[cache] WARNING: sensors promised to hosts already exceed cache_max_bytes \
                 ({kept} > {}); raise the limit or the cache will churn",
                self.max_bytes
            );
        }

        // Budget for the rest is the limit minus what we are obliged to keep
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

    /// How many bytes in-flight downloads occupy. They are never evicted
    /// (another task is writing them) but they do count against the limit —
    /// otherwise the cache would steadily overshoot `cache_max_bytes` by the
    /// size of the concurrent downloads.
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
                // A delay so concurrent calls genuinely overlap in time
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
        assert_eq!(dl.calls(), 1, "the second ensure must not download again");
    }

    #[tokio::test]
    async fn rejects_sha256_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let sha = sha256_of(b"expected");
        let dl = FakeDownloader::new(b"expected", true);
        let store = BinaryStore::new(dl, dir.path().to_path_buf(), u64::MAX);

        assert!(store.ensure(&sha).await.is_err());
        assert!(!dir.path().join(&sha).exists(), "a corrupt file must not be left behind");
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
            "ten concurrent requests = one download"
        );
    }

    #[tokio::test]
    async fn just_downloaded_file_survives_gc() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"a sensor larger than the limit";
        let sha = sha256_of(payload);
        let dl = FakeDownloader::new(payload, false);
        // The limit is smaller than the sensor — the GC must not remove what was just promised
        let store = BinaryStore::new(dl, dir.path().to_path_buf(), 4);

        let path = store.ensure(&sha).await.unwrap();
        assert!(path.exists(), "the just-downloaded file was evicted");
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
            "the /s/ route only accepts lowercase"
        );
        assert!(path.exists());
    }

    struct MapDownloader {
        payloads: HashMap<String, Vec<u8>>,
    }

    impl MapDownloader {
        fn new(payloads: &[&[u8]]) -> Arc<Self> {
            Arc::new(Self {
                payloads: payloads
                    .iter()
                    .map(|p| (sha256_of(p), p.to_vec()))
                    .collect(),
            })
        }
    }

    impl SensorDownloader for MapDownloader {
        fn fetch<'a>(&'a self, sha256: &'a str) -> BoxFuture<'a, Result<bytes::Bytes, AppError>> {
            Box::pin(async move {
                self.payloads
                    .get(sha256)
                    .map(|v| bytes::Bytes::from(v.clone()))
                    .ok_or_else(|| AppError::Other("no such sensor".into()))
            })
        }
    }

    #[tokio::test]
    async fn release_leaves_a_foreign_cell_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let store = BinaryStore::new(
            FakeDownloader::new(b"x", false),
            dir.path().to_path_buf(),
            u64::MAX,
        );

        // A cell from another attempt that waiters are still holding
        let live = Arc::new(OnceCell::new());
        store
            .inflight
            .lock()
            .unwrap()
            .insert("abc".into(), live.clone());

        // Our own cell is no longer the one in the map
        store.release("abc", &Arc::new(OnceCell::new()));

        let map = store.inflight.lock().unwrap();
        assert!(
            map.get("abc").is_some_and(|c| Arc::ptr_eq(c, &live)),
            "someone else's cell was dropped — the next request would start a second download"
        );
    }

    #[tokio::test]
    async fn release_removes_our_own_cell() {
        let dir = tempfile::tempdir().unwrap();
        let store = BinaryStore::new(
            FakeDownloader::new(b"x", false),
            dir.path().to_path_buf(),
            u64::MAX,
        );

        let ours = Arc::new(OnceCell::new());
        store
            .inflight
            .lock()
            .unwrap()
            .insert("abc".into(), ours.clone());
        store.release("abc", &ours);

        assert!(store.inflight.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn partial_file_is_removed_when_publishing_fails() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"sensor bytes";
        let sha = sha256_of(payload);
        // A directory under the final name — renaming a file over it will fail
        std::fs::create_dir(dir.path().join(&sha)).unwrap();

        let store = BinaryStore::new(
            FakeDownloader::new(payload, false),
            dir.path().to_path_buf(),
            u64::MAX,
        );

        // Straight to download_and_place: ensure() would see the directory,
        // treat the sensor as already cached and never attempt the rename
        assert!(store.download_and_place(&sha).await.is_err());

        let tmp = dir.path().join("tmp");
        assert_eq!(
            std::fs::read_dir(&tmp).unwrap().count(),
            0,
            "a leftover in tmp/ would count against the cache budget forever"
        );
    }

    #[tokio::test]
    async fn recently_promised_sensors_survive_gc() {
        let dir = tempfile::tempdir().unwrap();
        let first = b"first sensor payload";
        let second = b"second sensor payload";
        let dl = MapDownloader::new(&[first, second]);
        // The limit is just enough for a single sensor
        let store = BinaryStore::new(dl, dir.path().to_path_buf(), first.len() as u64);

        let a = store.ensure(&sha256_of(first)).await.unwrap();
        let b = store.ensure(&sha256_of(second)).await.unwrap();

        assert!(
            a.exists(),
            "a sensor promised to a host was evicted before the host arrived"
        );
        assert!(b.exists());
    }

    #[tokio::test]
    async fn files_nobody_was_promised_are_still_evicted() {
        let dir = tempfile::tempdir().unwrap();
        let payload = b"fresh sensor";
        let stale = dir.path().join("f".repeat(64));
        std::fs::write(&stale, vec![0u8; 1024]).unwrap();

        let store = BinaryStore::new(
            FakeDownloader::new(payload, false),
            dir.path().to_path_buf(),
            payload.len() as u64,
        );
        let fresh = store.ensure(&sha256_of(payload)).await.unwrap();

        assert!(!stale.exists(), "the stale file should have been evicted");
        assert!(fresh.exists());
    }

    // --- plan_eviction (pure function) ---

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
