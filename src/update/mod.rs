//! Самооновлення з GitHub Releases: перевірка нової версії, звірка
//! контрольної суми і заміна власного бінаря.

pub mod apply;
pub mod check;
pub mod verify;

pub use apply::apply_update;
pub use check::{check_for_update, UpdateInfo, CURRENT_VERSION};
