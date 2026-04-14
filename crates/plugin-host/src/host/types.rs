use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Embedded host.js runtime shipped with the binary.
pub(super) const HOST_JS: &str = include_str!("../../js/host.js");

/// A tool registered by a plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisteredTool {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

impl RegisteredTool {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn parameters(&self) -> &serde_json::Value {
        &self.parameters
    }
}

/// A command registered by a plugin.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisteredCommand {
    name: String,
    description: String,
}

impl RegisteredCommand {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Minimal execution context exposed to plugin handlers and commands.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct PluginContext {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, alias = "hasUI")]
    pub has_ui: bool,
    #[serde(default)]
    pub session_entries: Vec<serde_json::Value>,
    #[serde(default)]
    pub session_branch: Vec<serde_json::Value>,
    #[serde(default)]
    pub leaf_id: Option<String>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub session_file: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub session_name: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

/// A UI request from a plugin handler to the host (dialog or fire-and-forget).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiRequest {
    id: String,
    method: String,
    #[serde(flatten)]
    params: serde_json::Value,
}

impl UiRequest {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn params(&self) -> &serde_json::Value {
        &self.params
    }
}

/// A UI response from the host back to the plugin handler.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiResponse {
    id: String,
    #[serde(flatten)]
    data: serde_json::Value,
}

impl UiResponse {
    pub fn new(id: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            data,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn data(&self) -> &serde_json::Value {
        &self.data
    }
}

/// Handler for UI requests from plugins.
///
/// Implementors decide how to service each request:
/// - Print mode returns defaults (false for confirm, None for select/input, etc.)
/// - Interactive mode can show real TUI dialogs.
pub trait UiHandler: Send + Sync {
    fn handle_request(
        &self,
        request: UiRequest,
    ) -> Pin<Box<dyn Future<Output = UiResponse> + Send + '_>>;

    /// Downcast support for tests and introspection.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// A no-op UI handler that returns sensible defaults.
///
/// Used in print mode or when no UI is available.
#[derive(Clone, Debug, Default)]
pub struct DefaultUiHandler;

impl UiHandler for DefaultUiHandler {
    fn handle_request(
        &self,
        request: UiRequest,
    ) -> Pin<Box<dyn Future<Output = UiResponse> + Send + '_>> {
        Box::pin(async move { default_ui_response(&request) })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Produce a sensible default response for a UI request.
pub fn default_ui_response(request: &UiRequest) -> UiResponse {
    let data = match request.method() {
        "confirm" => serde_json::json!({ "confirmed": false }),
        "select" | "input" | "editor" => serde_json::json!({ "cancelled": true }),
        // Fire-and-forget methods: no meaningful response needed
        _ => serde_json::json!({}),
    };
    UiResponse::new(request.id(), data)
}

pub type SharedUiHandler = Arc<dyn UiHandler>;

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
