//! Self-update from GitHub Releases: checking for a new version, verifying
//! the checksum and replacing our own binary.

pub mod apply;
pub mod check;
pub mod verify;

pub use apply::apply_update;
pub use check::{check_for_update, UpdateInfo, CURRENT_VERSION};
