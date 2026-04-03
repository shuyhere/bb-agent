use serde::{Deserialize, Serialize};

/// Embedded host.js runtime shipped with the binary.
pub(super) const HOST_JS: &str = include_str!("../../js/host.js");

/// A tool registered by a plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisteredTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A command registered by a plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisteredCommand {
    pub name: String,
    pub description: String,
}

/// Minimal execution context exposed to plugin handlers and commands.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginContext {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, alias = "hasUI")]
    pub has_ui: bool,
}

/// Errors from the plugin host.
#[derive(Debug, thiserror::Error)]
pub enum PluginHostError {
    #[error("no plugins to load")]
    NoPlugins,
    #[error("failed to spawn plugin host: {0}")]
    SpawnFailed(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("plugin host process exited unexpectedly")]
    ProcessExited,
    #[error("timeout waiting for {0}")]
    Timeout(String),
}
