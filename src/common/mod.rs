//! Small utilities with no domain of their own: the error type, shutdown,
//! local addresses, token generation. Nothing here depends on the rest of
//! the crate — dependencies only point inwards.

pub mod error;
pub mod net;
pub mod shutdown;
pub mod token;

pub use error::AppError;
