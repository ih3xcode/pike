//! `pike serve` configuration: command-line arguments, the TOML file and
//! the merge of both into a single immutable [`ResolvedConfig`].
//!
//! Precedence: flags > environment variables > file > defaults.

pub mod args;
pub mod defaults;
pub mod file;
pub mod resolve;
pub mod validate;

pub use args::ServeArgs;
pub use file::{load_file, FileConfig};
pub use resolve::{resolve, ResolvedConfig};
pub use validate::validate_token;
