use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use crate::common::error::AppError;

use super::ports::SensorLister;
use super::types::SensorMeta;

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
pub struct MetadataCache {
    lister: Arc<dyn SensorLister>,
    ttl: Duration,
    state: tokio::sync::Mutex<CacheState>,
}

impl MetadataCache {
    pub fn new(lister: Arc<dyn SensorLister>, ttl: Duration) -> Self {
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
        let lister = FakeLister::new(1); // перший виклик успішний, далі помилки
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
        let cache = cache_with(lister.clone(), 60);

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
        let cache = cache_with(lister.clone(), 60);

        assert!(cache.get("linux").await.is_err());
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
