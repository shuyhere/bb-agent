use thiserror::Error;

/// Domain errors for the session crate.
#[derive(Debug, Error)]
pub enum SessionError {
    #[error("database error: {0}")]
    DatabaseError(String),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("compaction error: {0}")]
    CompactionError(String),

    #[error("import/export error: {0}")]
    ImportExportError(String),
}

pub type Result<T> = std::result::Result<T, SessionError>;
