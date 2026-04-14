use tracing::{debug, warn};

use super::super::{PluginHost, types::UiRequest};

impl PluginHost {
    /// Handle an incoming UI request from a plugin handler.
    ///
    /// Forwards to the registered UI handler (or produces a default response),
    /// then sends the response back to the JS host as a `ui_response` message.
    pub(super) async fn handle_ui_request_inline(&mut self, params: serde_json::Value) {
        let request = match serde_json::from_value::<UiRequest>(params) {
            Ok(r) => r,
            Err(e) => {
                warn!("Invalid ui_request params: {e}");
                return;
            }
        };

        let is_fire_and_forget = matches!(
            request.method(),
            "notify" | "setStatus" | "setWidget" | "setTitle" | "set_editor_text"
        );

        let response = if let Some(handler) = &self.ui_handler {
            handler.handle_request(request.clone()).await
        } else {
            super::super::types::default_ui_response(&request)
        };

        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "ui_response",
            "params": response,
        });

        if let Err(e) = self.send_json(&msg).await {
            warn!("Failed to send ui_response: {e}");
        }

        if is_fire_and_forget {
            debug!("Handled fire-and-forget UI request: {}", request.method());
        } else {
            debug!("Handled dialog UI request: {}", request.method());
        }
    }
}
