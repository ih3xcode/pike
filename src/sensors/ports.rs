use std::future::Future;
use std::pin::Pin;

use crate::common::error::AppError;

use super::types::SensorMeta;

/// Обʼєктно-безпечне майбутнє. `impl Future` у трейті було б дешевшим,
/// але робить трейт не-dyn-сумісним, а кешам потрібен саме `Arc<dyn …>`:
/// інакше параметр типу протікає в `AppState` і далі в увесь сервер і GUI.
/// `+ Send` обовʼязковий — axum вимагає Send-хендлери.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Джерело списку доступних сенсорів.
pub trait SensorLister: Send + Sync + 'static {
    fn list<'a>(&'a self, platform: &'a str) -> BoxFuture<'a, Result<Vec<SensorMeta>, AppError>>;
}

/// Джерело байтів сенсора за його sha256.
pub trait SensorDownloader: Send + Sync + 'static {
    fn fetch<'a>(&'a self, sha256: &'a str) -> BoxFuture<'a, Result<bytes::Bytes, AppError>>;
}
