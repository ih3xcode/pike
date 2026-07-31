use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },

    #[error("{context}: {source}")]
    Http {
        context: String,
        source: reqwest::Error,
    },

    #[error("{context} (HTTP {status}): {body}")]
    ApiResponse {
        context: String,
        status: u16,
        body: String,
    },

    #[error("{0}")]
    Other(String),
}

impl AppError {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub fn http(context: impl Into<String>, source: reqwest::Error) -> Self {
        Self::Http {
            context: context.into(),
            source,
        }
    }

    pub fn api(context: impl Into<String>, status: reqwest::StatusCode, body: String) -> Self {
        Self::ApiResponse {
            context: context.into(),
            status: status.as_u16(),
            body,
        }
    }
}
