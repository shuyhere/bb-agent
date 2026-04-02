use thiserror::Error;

/// Domain errors for the hooks crate.
#[derive(Debug, Error)]
pub enum HookError {
    #[error("handler failed: {0}")]
    HandlerFailed(String),

    #[error("hook timed out")]
    Timeout,
}

pub type Result<T> = std::result::Result<T, HookError>;
