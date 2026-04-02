use thiserror::Error;

/// Domain errors for the plugin-host crate.
///
/// Note: `PluginHostError` in `host.rs` is the existing error type used
/// at runtime. This enum covers additional higher-level failure modes.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin launch failed: {0}")]
    LaunchFailed(String),

    #[error("protocol error: {0}")]
    ProtocolError(String),

    #[error("plugin timed out: {0}")]
    Timeout(String),

    #[error("plugin not found: {0}")]
    NotFound(String),

    #[error("plugin host error: {0}")]
    Host(#[from] crate::PluginHostError),
}

pub type Result<T> = std::result::Result<T, PluginError>;
