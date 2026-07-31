//! Усе, що стосується сенсорів: їхні типи, зіставлення з хостом,
//! завантаження локальних файлів і два кеші поверх зовнішнього джерела.
//!
//! Джерело описане тут же — трейтами в [`ports`]. Реалізацію дає `falcon`,
//! тому залежність іде лише в один бік: `falcon` знає про `sensors`,
//! `sensors` про `falcon` — ні.

pub mod binary_store;
pub mod loading;
pub mod matching;
pub mod metadata_cache;
pub mod ports;
pub mod types;

pub use binary_store::BinaryStore;
pub use metadata_cache::MetadataCache;
pub use types::{Sensor, SensorType};
