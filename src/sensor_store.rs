use std::future::Future;

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
