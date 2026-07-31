use std::future::Future;
use std::pin::Pin;

use crate::common::error::AppError;

use super::types::SensorMeta;

/// An object-safe future. `impl Future` in the trait would be cheaper, but
/// it makes the trait not dyn-compatible, and the caches need `Arc<dyn …>`:
/// otherwise the type parameter leaks into `AppState` and from there into
/// the whole server and GUI. `+ Send` is mandatory — axum requires Send
/// handlers.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A source for the list of available sensors.
pub trait SensorLister: Send + Sync + 'static {
    fn list<'a>(&'a self, platform: &'a str) -> BoxFuture<'a, Result<Vec<SensorMeta>, AppError>>;
}

/// A sensor's bytes as a stream. Sensors are hundreds of megabytes, so the
/// port hands back a reader rather than a `Bytes`: buffering whole installers
/// meant peak memory scaled with the number of concurrent distinct downloads.
pub type BoxReader<'a> = Pin<Box<dyn tokio::io::AsyncRead + Send + 'a>>;

/// A source for a sensor's bytes, keyed by its sha256.
pub trait SensorDownloader: Send + Sync + 'static {
    fn fetch<'a>(&'a self, sha256: &'a str) -> BoxFuture<'a, Result<BoxReader<'a>, AppError>>;
}
