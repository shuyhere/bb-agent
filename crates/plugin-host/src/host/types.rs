use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

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
    pub id: String,
    pub method: String,
    #[serde(flatten)]
    pub params: serde_json::Value,
}

/// A UI response from the host back to the plugin handler.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiResponse {
    pub id: String,
    #[serde(flatten)]
    pub data: serde_json::Value,
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
    let data = match request.method.as_str() {
        "confirm" => serde_json::json!({ "confirmed": false }),
        "select" | "input" | "editor" => serde_json::json!({ "cancelled": true }),
        // Fire-and-forget methods: no meaningful response needed
        _ => serde_json::json!({}),
    };
    UiResponse {
        id: request.id.clone(),
        data,
    }
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
