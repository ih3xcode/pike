//! Everything about sensors: their types, matching against a host, loading
//! local files, and two caches over an external source.
//!
//! That source is described right here, by the traits in [`ports`]. `falcon`
//! provides the implementation, so the dependency runs one way only:
//! `falcon` knows about `sensors`, `sensors` knows nothing about `falcon`.

pub mod binary_store;
pub mod loading;
pub mod matching;
pub mod metadata_cache;
pub mod ports;
pub mod types;

pub use binary_store::BinaryStore;
pub use metadata_cache::MetadataCache;
pub use types::{Sensor, SensorType};
