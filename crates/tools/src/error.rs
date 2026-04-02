use thiserror::Error;

/// Domain errors for the tools crate.
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("execution failed: {0}")]
    ExecutionFailed(String),

    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    #[error("tool execution timed out")]
    Timeout,

    #[error("tool not found: {0}")]
    NotFound(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ToolError>;
