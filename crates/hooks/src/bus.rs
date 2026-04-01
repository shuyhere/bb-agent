use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::events::{Event, HookResult};

/// Handler function type.
pub type HandlerFn = Arc<dyn Fn(&Event) -> Option<HookResult> + Send + Sync>;

struct HandlerEntry {
    plugin_id: String,
    handler: HandlerFn,
}

/// Event bus for dispatching hook events.
pub struct EventBus {
    handlers: RwLock<HashMap<String, Vec<HandlerEntry>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a handler for an event type.
    pub async fn on(&self, event_type: &str, plugin_id: &str, handler: HandlerFn) {
        let mut handlers = self.handlers.write().await;
        handlers
            .entry(event_type.to_string())
            .or_default()
            .push(HandlerEntry {
                plugin_id: plugin_id.to_string(),
                handler,
            });
    }

    /// Check if any handlers are registered for an event type.
    pub async fn has_handlers(&self, event_type: &str) -> bool {
        let handlers = self.handlers.read().await;
        handlers
            .get(event_type)
            .map(|h| !h.is_empty())
            .unwrap_or(false)
    }

    /// Emit an event, running all registered handlers.
    /// Returns the merged result (last non-None wins for each field).
    pub async fn emit(&self, event: &Event) -> Option<HookResult> {
        let event_type = event.event_type();
        let handlers = self.handlers.read().await;

        let entries = match handlers.get(event_type) {
            Some(e) => e,
            None => return None,
        };

        let mut merged = HookResult::default();
        let mut any_result = false;

        for entry in entries {
            if let Some(result) = (entry.handler)(event) {
                any_result = true;
                // Merge: last non-None wins
                if result.block.is_some() {
                    merged.block = result.block;
                }
                if result.reason.is_some() {
                    merged.reason = result.reason;
                }
                if result.cancel.is_some() {
                    merged.cancel = result.cancel;
                }
                if result.messages.is_some() {
                    merged.messages = result.messages;
                }
                if result.system_prompt.is_some() {
                    merged.system_prompt = result.system_prompt;
                }
                if result.message.is_some() {
                    merged.message = result.message;
                }
                if result.content.is_some() {
                    merged.content = result.content;
                }
                if result.action.is_some() {
                    merged.action = result.action;
                }
                if result.text.is_some() {
                    merged.text = result.text;
                }
                if result.payload.is_some() {
                    merged.payload = result.payload;
                }

                // If blocked or cancelled, stop processing
                if merged.block == Some(true) || merged.cancel == Some(true) {
                    break;
                }
            }
        }

        if any_result {
            Some(merged)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::*;

    #[tokio::test]
    async fn test_emit_no_handlers() {
        let bus = EventBus::new();
        let result = bus.emit(&Event::SessionStart).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_emit_with_handler() {
        let bus = EventBus::new();
        bus.on(
            "session_start",
            "test-plugin",
            Arc::new(|_| {
                Some(HookResult {
                    action: Some("handled".to_string()),
                    ..Default::default()
                })
            }),
        )
        .await;

        let result = bus.emit(&Event::SessionStart).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().action, Some("handled".to_string()));
    }

    #[tokio::test]
    async fn test_tool_call_block() {
        let bus = EventBus::new();
        bus.on(
            "tool_call",
            "safety",
            Arc::new(|event| {
                if let Event::ToolCall(tc) = event {
                    if tc.tool_name == "bash" && tc.input.to_string().contains("rm -rf") {
                        return Some(HookResult {
                            block: Some(true),
                            reason: Some("Dangerous command".into()),
                            ..Default::default()
                        });
                    }
                }
                None
            }),
        )
        .await;

        let event = Event::ToolCall(ToolCallEvent {
            tool_call_id: "1".into(),
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "rm -rf /"}),
        });

        let result = bus.emit(&event).await.unwrap();
        assert_eq!(result.block, Some(true));
        assert_eq!(result.reason, Some("Dangerous command".into()));
    }
}
