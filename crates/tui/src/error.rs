use thiserror::Error;

/// Domain errors for the TUI crate.
#[derive(Debug, Error)]
pub enum TuiError {
    #[error("render error: {0}")]
    RenderError(String),

    #[error("terminal error: {0}")]
    TerminalError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TuiError>;
