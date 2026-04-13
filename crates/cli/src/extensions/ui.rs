use std::collections::BTreeMap;
use std::sync::Arc;

use bb_plugin_host::{UiHandler, UiRequest, UiResponse, default_ui_response};
use tokio::sync::Mutex;

/// Print-mode UI handler: logs notifications to tracing, returns defaults for dialogs.
#[derive(Clone, Debug, Default)]
pub(crate) struct PrintUiHandler;

impl UiHandler for PrintUiHandler {
    fn handle_request(
        &self,
        request: UiRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = UiResponse> + Send + '_>> {
        Box::pin(async move {
            match request.method.as_str() {
                "notify" => {
                    let msg = request
                        .params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let kind = request
                        .params
                        .get("notifyType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info");
                    match kind {
                        "error" => tracing::error!("[extension] {msg}"),
                        "warning" => tracing::warn!("[extension] {msg}"),
                        _ => tracing::info!("[extension] {msg}"),
                    }
                }
                "setStatus" | "setWidget" | "setTitle" | "set_editor_text" => {
                    // No-op in print mode
                }
                _ => {}
            }
            default_ui_response(&request)
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Shared extension UI handler for TUI mode. It stores notifications and
/// statuses so the active UI can surface them if needed.
#[derive(Clone, Debug)]
pub(crate) struct ExtensionUiHandler {
    notifications: Arc<Mutex<Vec<UiNotification>>>,
    statuses: Arc<Mutex<BTreeMap<String, Option<String>>>>,
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(crate) struct UiNotification {
    pub message: String,
    pub kind: String,
}

impl Default for ExtensionUiHandler {
    fn default() -> Self {
        Self {
            notifications: Arc::new(Mutex::new(Vec::new())),
            statuses: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

#[cfg(test)]
impl ExtensionUiHandler {
    /// Drain all pending notifications.
    pub(crate) async fn drain_notifications(&self) -> Vec<UiNotification> {
        let mut notifications = self.notifications.lock().await;
        std::mem::take(&mut *notifications)
    }

    /// Get all current status entries.
    pub(crate) async fn get_statuses(&self) -> BTreeMap<String, Option<String>> {
        self.statuses.lock().await.clone()
    }
}

impl UiHandler for ExtensionUiHandler {
    fn handle_request(
        &self,
        request: UiRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = UiResponse> + Send + '_>> {
        let notifications = self.notifications.clone();
        let statuses = self.statuses.clone();
        Box::pin(async move {
            match request.method.as_str() {
                "notify" => {
                    let msg = request
                        .params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let kind = request
                        .params
                        .get("notifyType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string();
                    notifications
                        .lock()
                        .await
                        .push(UiNotification { message: msg, kind });
                }
                "setStatus" => {
                    let key = request
                        .params
                        .get("statusKey")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let text = request
                        .params
                        .get("statusText")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    statuses.lock().await.insert(key, text);
                }
                "confirm" | "select" | "input" | "editor" => {
                    // Tui currently does not surface extension dialogs inline.
                    // Fall back to the default canned response behavior.
                }
                "setWidget" | "setTitle" | "set_editor_text" => {
                    tracing::debug!("Extension UI event: {}", request.method);
                }
                _ => {}
            }
            default_ui_response(&request)
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
