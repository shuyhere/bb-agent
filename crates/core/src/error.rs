use thiserror::Error;

#[derive(Error, Debug)]
pub enum BbError {
    #[error("Session error: {0}")]
    Session(String),

    #[error("Entry not found: {0}")]
    EntryNotFound(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Aborted")]
    Aborted,
}

pub type BbResult<T> = Result<T, BbError>;
