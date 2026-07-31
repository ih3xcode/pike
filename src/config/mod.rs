//! Конфігурація `pike serve`: аргументи командного рядка, TOML-файл
//! і злиття їх в один незмінний [`ResolvedConfig`].
//!
//! Пріоритет: прапорці > змінні середовища > файл > дефолти.

pub mod args;
pub mod defaults;
pub mod file;
pub mod resolve;
pub mod validate;

pub use args::ServeArgs;
pub use file::{load_file, FileConfig};
pub use resolve::{resolve, ResolvedConfig};
pub use validate::validate_token;
