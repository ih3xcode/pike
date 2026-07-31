use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use crate::common::error::AppError;

use super::ports::SensorLister;
use super::types::SensorMeta;

/// Minimum pause between attempts after a failure. Without it, every /cb
/// request would pay the full network timeout while the API is unreachable.
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60);

struct Snapshot {
    metas: Vec<SensorMeta>,
    fetched_at: Instant,
}

#[derive(Default)]
struct CacheState {
    snapshots: HashMap<String, Snapshot>,
    /// When the last request for a platform failed. Kept separate from the
    /// snapshots on purpose: on a cold start there is no snapshot yet, and
    /// that is exactly when the retry pause matters most.
    failures: HashMap<String, Instant>,
}

/// The API sensor list with a finite lifetime.
///
/// Freshness here is the main guard against versions freezing: matching
/// always runs against the current list, not against whatever once landed
/// in the cache.
pub struct MetadataCache {
    lister: Arc<dyn SensorLister>,
    ttl: Duration,
    /// Never held across an await, so reading a fresh snapshot never waits
    /// on anyone's network call.
    state: std::sync::Mutex<CacheState>,
    /// One refresh at a time *per platform*. A single lock for everything
    /// meant a slow Linux listing — up to the 30s metadata timeout — queued
    /// every concurrent Windows callback behind it, snapshot or no snapshot.
    gates: std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl MetadataCache {
    pub fn new(lister: Arc<dyn SensorLister>, ttl: Duration) -> Self {
        Self {
            lister,
            ttl,
            state: std::sync::Mutex::new(CacheState::default()),
            gates: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub async fn get(&self, platform: &str) -> Result<Vec<SensorMeta>, AppError> {
        if let Some(answer) = self.cached(platform) {
            return answer;
        }

        let gate = self.gate(platform);
        let _refreshing = gate.lock().await;

        // Someone may have refreshed this platform while we waited
        if let Some(answer) = self.cached(platform) {
            return answer;
        }

        let result = self.lister.list(platform).await;
        self.store(platform, result)
    }

    /// What the cache can answer without touching the API: a snapshot within
    /// its TTL, or whatever the failure window dictates. `None` means fetch.
    fn cached(&self, platform: &str) -> Option<Result<Vec<SensorMeta>, AppError>> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();

        if let Some(snapshot) = state.snapshots.get(platform) {
            if now.duration_since(snapshot.fetched_at) < self.ttl {
                return Some(Ok(snapshot.metas.clone()));
            }
        }

        // A recent failure — do not hit the API again, whether or not there
        // is something in the cache to serve
        let failed_at = state.failures.get(platform)?;
        if now.duration_since(*failed_at) >= RETRY_AFTER_FAILURE {
            return None;
        }
        Some(match state.snapshots.get(platform) {
            Some(snapshot) => Ok(snapshot.metas.clone()),
            None => Err(AppError::Other(format!(
                "sensor list for '{platform}' unavailable; \
                 the last API call failed and the retry window has not elapsed"
            ))),
        })
    }

    fn store(
        &self,
        platform: &str,
        result: Result<Vec<SensorMeta>, AppError>,
    ) -> Result<Vec<SensorMeta>, AppError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(metas) => {
                let out = metas.clone();
                state.failures.remove(platform);
                state.snapshots.insert(
                    platform.to_string(),
                    Snapshot {
                        metas,
                        fetched_at: Instant::now(),
                    },
                );
                Ok(out)
            }
            Err(e) => {
                state.failures.insert(platform.to_string(), Instant::now());
                match state.snapshots.get(platform) {
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

    fn gate(&self, platform: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut gates = self.gates.lock().unwrap_or_else(|e| e.into_inner());
        gates.entry(platform.to_string()).or_default().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensors::ports::BoxFuture;
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
        fn list<'a>(
            &'a self,
            _platform: &'a str,
        ) -> BoxFuture<'a, Result<Vec<SensorMeta>, AppError>> {
            Box::pin(async move {
                let n = self.calls.fetch_add(1, Ordering::SeqCst);
                if n >= self.fail_after {
                    Err(AppError::Other("api down".into()))
                } else {
                    Ok(vec![meta(&format!("sensor{n}.deb"))])
                }
            })
        }
    }

    fn cache_with(lister: Arc<FakeLister>, ttl_secs: u64) -> MetadataCache {
        MetadataCache::new(lister, Duration::from_secs(ttl_secs))
    }

    #[tokio::test(start_paused = true)]
    async fn fetches_once_then_serves_from_cache() {
        let lister = FakeLister::new(usize::MAX);
        let cache = cache_with(lister.clone(), 3600);

        let a = cache.get("linux").await.unwrap();
        let b = cache.get("linux").await.unwrap();

        assert_eq!(lister.calls(), 1);
        assert_eq!(a[0].name, b[0].name);
    }

    #[tokio::test(start_paused = true)]
    async fn refetches_after_ttl() {
        let lister = FakeLister::new(usize::MAX);
        let cache = cache_with(lister.clone(), 60);

        cache.get("linux").await.unwrap();
        tokio::time::advance(Duration::from_secs(61)).await;
        let second = cache.get("linux").await.unwrap();

        assert_eq!(lister.calls(), 2);
        assert_eq!(second[0].name, "sensor1.deb");
    }

    #[tokio::test(start_paused = true)]
    async fn serves_stale_snapshot_when_api_fails() {
        let lister = FakeLister::new(1); // first call succeeds, the rest fail
        let cache = cache_with(lister.clone(), 60);

        cache.get("linux").await.unwrap();
        tokio::time::advance(Duration::from_secs(61)).await;
        let stale = cache.get("linux").await.unwrap();

        assert_eq!(stale[0].name, "sensor0.deb");
        assert_eq!(lister.calls(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn negative_caching_suppresses_immediate_retry() {
        let lister = FakeLister::new(1);
        let cache = cache_with(lister.clone(), 60);

        cache.get("linux").await.unwrap();
        tokio::time::advance(Duration::from_secs(61)).await;
        cache.get("linux").await.unwrap(); // fails, serves the stale snapshot
        cache.get("linux").await.unwrap(); // must not hit the API again

        assert_eq!(lister.calls(), 2);

        tokio::time::advance(Duration::from_secs(61)).await;
        cache.get("linux").await.unwrap();
        assert_eq!(lister.calls(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn negative_caching_applies_without_any_snapshot() {
        // Cold start with an unreachable API: without the pause every /cb
        // request would pay the full network timeout
        let lister = FakeLister::new(0);
        let cache = cache_with(lister.clone(), 60);

        assert!(cache.get("linux").await.is_err());
        assert!(cache.get("linux").await.is_err());
        assert!(cache.get("linux").await.is_err());
        assert_eq!(lister.calls(), 1, "the retries should have hit the pause");

        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(cache.get("linux").await.is_err());
        assert_eq!(lister.calls(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn errors_when_no_snapshot_at_all() {
        let lister = FakeLister::new(0); // fails immediately
        let cache = cache_with(lister.clone(), 60);

        assert!(cache.get("linux").await.is_err());
    }

    struct SlowLister {
        delay: Duration,
    }

    impl SensorLister for SlowLister {
        fn list<'a>(
            &'a self,
            platform: &'a str,
        ) -> BoxFuture<'a, Result<Vec<SensorMeta>, AppError>> {
            Box::pin(async move {
                if platform == "linux" {
                    tokio::time::sleep(self.delay).await;
                }
                Ok(vec![meta(platform)])
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_slow_platform_does_not_block_another() {
        let cache = Arc::new(MetadataCache::new(
            Arc::new(SlowLister {
                delay: Duration::from_secs(30),
            }),
            Duration::from_secs(3600),
        ));

        let slow = tokio::spawn({
            let cache = cache.clone();
            async move { cache.get("linux").await }
        });
        // Let the Linux fetch take its gate and settle into the sleep
        tokio::time::sleep(Duration::from_millis(10)).await;

        let start = Instant::now();
        cache.get("windows").await.unwrap();
        assert_eq!(
            start.elapsed(),
            Duration::ZERO,
            "a stalled listing for one platform must not queue another"
        );

        slow.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn platforms_are_cached_independently() {
        let lister = FakeLister::new(usize::MAX);
        let cache = cache_with(lister.clone(), 3600);

        cache.get("linux").await.unwrap();
        cache.get("windows").await.unwrap();

        assert_eq!(lister.calls(), 2);
    }
}
