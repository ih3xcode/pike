use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

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

struct PlatformState {
    metas: Vec<SensorMeta>,
    fetched_at: Instant,
    last_failure: Option<Instant>,
}

/// Список сенсорів з API зі скінченним часом життя.
///
/// Свіжість тут — головний захист від застигання версій: матчинг завжди
/// відбувається по актуальному списку, а не по тому, що колись потрапило в кеш.
pub struct MetadataCache<L: SensorLister> {
    lister: Arc<L>,
    ttl: Duration,
    state: tokio::sync::Mutex<HashMap<String, PlatformState>>,
}

impl<L: SensorLister> MetadataCache<L> {
    pub fn new(lister: Arc<L>, ttl: Duration) -> Self {
        Self {
            lister,
            ttl,
            state: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub async fn get(&self, platform: &str) -> Result<Vec<SensorMeta>, AppError> {
        // Лок тримається через await навмисно: це серіалізує паралельні
        // оновлення одного платформного списку в один запит до API.
        let mut guard = self.state.lock().await;
        let now = Instant::now();

        if let Some(entry) = guard.get(platform) {
            if now.duration_since(entry.fetched_at) < self.ttl {
                return Ok(entry.metas.clone());
            }
            if let Some(failed_at) = entry.last_failure {
                if now.duration_since(failed_at) < RETRY_AFTER_FAILURE {
                    return Ok(entry.metas.clone());
                }
            }
        }

        match self.lister.list(platform).await {
            Ok(metas) => {
                let result = metas.clone();
                guard.insert(
                    platform.to_string(),
                    PlatformState {
                        metas,
                        fetched_at: Instant::now(),
                        last_failure: None,
                    },
                );
                Ok(result)
            }
            Err(e) => {
                if let Some(entry) = guard.get_mut(platform) {
                    entry.last_failure = Some(Instant::now());
                    eprintln!(
                        "[meta] WARNING: refresh for '{platform}' failed ({e}); \
                         serving snapshot from cache"
                    );
                    return Ok(entry.metas.clone());
                }
                Err(e)
            }
        }
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
}
