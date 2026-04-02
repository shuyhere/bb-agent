use thiserror::Error;

/// Domain errors for the provider crate.
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("HTTP error {status}: {body}")]
    HttpError { status: u16, body: String },

    #[error("stream error: {0}")]
    StreamError(String),

    #[error("authentication error: {0}")]
    AuthError(String),

    #[error("failed to parse response: {0}")]
    ParseError(String),

    #[error("unsupported model: {0}")]
    UnsupportedModel(String),

    #[error("request failed: {0}")]
    RequestFailed(String),
}

pub type Result<T> = std::result::Result<T, ProviderError>;
