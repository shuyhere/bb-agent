use thiserror::Error;

/// Domain errors for the CLI crate.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum CliError {
    #[error("authentication error: {0}")]
    AuthError(String),

    #[error("configuration error: {0}")]
    ConfigError(String),

    #[error("session error: {0}")]
    SessionError(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, CliError>;
