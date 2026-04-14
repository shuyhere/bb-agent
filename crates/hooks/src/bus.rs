use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::warn;

use crate::events::{Event, HookResult};

/// Shared, cloneable event bus handle.
pub type SharedEventBus = Arc<EventBus>;

/// Handler function type.
pub type HandlerFn = Arc<dyn Fn(&Event) -> Option<HookResult> + Send + Sync>;

struct HandlerEntry {
    plugin_id: String,
    handler: HandlerFn,
}

/// Event bus for dispatching hook events.
///
/// Handlers are grouped by the event's wire name (`Event::event_type()`). Results are
/// merged with a last-non-`None`-wins policy, and a blocking/cancelling result stops
/// further dispatch for that event.
pub struct EventBus {
    handlers: RwLock<HashMap<String, Vec<HandlerEntry>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Create an empty event bus.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(HashMap::new()),
        }
    }

    /// Create a new [`EventBus`] wrapped in [`Arc`] for shared ownership.
    #[must_use]
    pub fn shared() -> SharedEventBus {
        Arc::new(Self::new())
    }

    /// Register a handler for an event type.
    ///
    /// The `plugin_id` is retained for diagnostics so panics can be attributed to the
    /// plugin that caused them.
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
    ///
    /// Returns the merged result if at least one handler produced a value. A panicking
    /// handler is logged and skipped so one extension cannot take down all hook
    /// dispatch.
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
            let invocation = catch_unwind(AssertUnwindSafe(|| (entry.handler)(event)));
            let Some(result) = (match invocation {
                Ok(result) => result,
                Err(_) => {
                    warn!(
                        event_type,
                        plugin_id = %entry.plugin_id,
                        "hook handler panicked; skipping result"
                    );
                    continue;
                }
            }) else {
                continue;
            };

            any_result = true;
            merged.merge_from(result);

            if merged.stops_dispatch() {
                break;
            }
        }

        if any_result { Some(merged) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::events::*;

    #[tokio::test]
    async fn test_emit_no_handlers() {
        let bus = EventBus::new();
        let result = bus.emit(&Event::SessionStart).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_has_handlers_reflects_registration() {
        let bus = EventBus::new();
        assert!(!bus.has_handlers("session_start").await);

        bus.on(
            "session_start",
            "test-plugin",
            Arc::new(|_| Some(HookResult::default())),
        )
        .await;

        assert!(bus.has_handlers("session_start").await);
        assert!(!bus.has_handlers("tool_call").await);
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
    async fn test_later_handlers_override_previous_fields() {
        let bus = EventBus::new();
        bus.on(
            "session_start",
            "first",
            Arc::new(|_| {
                Some(HookResult {
                    action: Some("first".to_string()),
                    text: Some("initial".to_string()),
                    ..Default::default()
                })
            }),
        )
        .await;
        bus.on(
            "session_start",
            "second",
            Arc::new(|_| {
                Some(HookResult {
                    text: Some("final".to_string()),
                    ..Default::default()
                })
            }),
        )
        .await;

        let result = bus.emit(&Event::SessionStart).await.unwrap();
        assert_eq!(result.action.as_deref(), Some("first"));
        assert_eq!(result.text.as_deref(), Some("final"));
    }

    #[tokio::test]
    async fn test_panicking_handler_is_skipped() {
        let bus = EventBus::new();
        bus.on("session_start", "panicker", Arc::new(|_| panic!("boom")))
            .await;
        bus.on(
            "session_start",
            "survivor",
            Arc::new(|_| {
                Some(HookResult {
                    action: Some("still-ran".to_string()),
                    ..Default::default()
                })
            }),
        )
        .await;

        let result = bus.emit(&Event::SessionStart).await.unwrap();
        assert_eq!(result.action.as_deref(), Some("still-ran"));
    }

    #[tokio::test]
    async fn test_tool_call_block() {
        let bus = EventBus::new();
        bus.on(
            "tool_call",
            "safety",
            Arc::new(|event| {
                if let Event::ToolCall(tc) = event
                    && tc.tool_name() == "bash"
                    && tc.input().to_string().contains("rm -rf")
                {
                    return Some(HookResult {
                        block: Some(true),
                        reason: Some("Dangerous command".into()),
                        ..Default::default()
                    });
                }
                None
            }),
        )
        .await;

        let event = Event::ToolCall(ToolCallEvent::new(
            "1",
            "bash",
            serde_json::json!({"command": "rm -rf /"}),
        ));

        let result = bus.emit(&event).await.unwrap();
        assert_eq!(result.block, Some(true));
        assert_eq!(result.reason, Some("Dangerous command".into()));
    }

    #[tokio::test]
    async fn test_blocking_result_stops_later_handlers() {
        let bus = EventBus::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let later_count = Arc::new(AtomicUsize::new(0));

        let first_count = call_count.clone();
        bus.on(
            "tool_call",
            "blocker",
            Arc::new(move |_| {
                first_count.fetch_add(1, Ordering::SeqCst);
                Some(HookResult {
                    block: Some(true),
                    reason: Some("stop".into()),
                    ..Default::default()
                })
            }),
        )
        .await;

        let second_count = later_count.clone();
        bus.on(
            "tool_call",
            "later",
            Arc::new(move |_| {
                second_count.fetch_add(1, Ordering::SeqCst);
                Some(HookResult {
                    action: Some("should-not-run".into()),
                    ..Default::default()
                })
            }),
        )
        .await;

        let _ = bus
            .emit(&Event::ToolCall(ToolCallEvent::new(
                "1",
                "bash",
                serde_json::json!({"command": "pwd"}),
            )))
            .await;

        assert_eq!(call_count.load(Ordering::SeqCst), 1);
        assert_eq!(later_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_context_hook_modifies_messages() {
        use bb_core::types::{AgentMessage, ContentBlock, UserMessage};

        let bus = EventBus::new();
        bus.on(
            "context",
            "context-manager",
            Arc::new(|event| {
                if let Event::Context(_ctx) = event {
                    let replacement = serde_json::json!([
                        { "User": { "content": [{ "Text": { "text": "replaced" } }], "timestamp": 0 } }
                    ]);
                    if let serde_json::Value::Array(arr) = replacement {
                        return Some(HookResult {
                            messages: Some(arr),
                            ..Default::default()
                        });
                    }
                }
                None
            }),
        )
        .await;

        let original_messages = vec![AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "original".into(),
            }],
            timestamp: 0,
        })];

        let event = Event::Context(ContextEvent::new(original_messages));
        let result = bus.emit(&event).await.unwrap();
        assert!(result.messages.is_some());
        let msgs = result.messages.unwrap();
        assert_eq!(msgs.len(), 1);
    }

    #[tokio::test]
    async fn test_before_agent_start_modifies_prompt() {
        let bus = EventBus::new();
        bus.on(
            "before_agent_start",
            "prompt-modifier",
            Arc::new(|event| {
                if let Event::BeforeAgentStart { system_prompt, .. } = event {
                    let modified = format!("{system_prompt}\n\nAdditional instructions.");
                    return Some(HookResult {
                        system_prompt: Some(modified),
                        ..Default::default()
                    });
                }
                None
            }),
        )
        .await;

        let event = Event::BeforeAgentStart {
            prompt: "Hello".into(),
            system_prompt: "You are a helpful assistant.".into(),
        };
        let result = bus.emit(&event).await.unwrap();
        assert_eq!(
            result.system_prompt,
            Some("You are a helpful assistant.\n\nAdditional instructions.".into()),
        );
    }

    #[tokio::test]
    async fn test_shared_event_bus() {
        let bus = EventBus::shared();
        bus.on(
            "session_start",
            "test",
            Arc::new(|_| {
                Some(HookResult {
                    action: Some("started".into()),
                    ..Default::default()
                })
            }),
        )
        .await;

        let bus2 = bus.clone();
        let result = bus2.emit(&Event::SessionStart).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().action, Some("started".into()));
    }

    #[tokio::test]
    async fn test_session_before_compact_cancel() {
        let bus = EventBus::new();
        bus.on(
            "session_before_compact",
            "compaction-guard",
            Arc::new(|_| {
                Some(HookResult {
                    cancel: Some(true),
                    reason: Some("Compaction disabled by extension".into()),
                    ..Default::default()
                })
            }),
        )
        .await;

        let event = Event::SessionBeforeCompact(CompactPrep::new("abc123", 5000));
        let result = bus.emit(&event).await.unwrap();
        assert_eq!(result.cancel, Some(true));
    }

    #[tokio::test]
    async fn test_tool_result_hook_replaces_content() {
        let bus = EventBus::new();
        bus.on(
            "tool_result",
            "content-filter",
            Arc::new(|event| {
                if let Event::ToolResult(tr) = event
                    && tr.tool_name() == "bash"
                {
                    return Some(HookResult {
                        content: Some(vec![
                            serde_json::json!({ "Text": { "text": "[redacted]" } }),
                        ]),
                        ..Default::default()
                    });
                }
                None
            }),
        )
        .await;

        let event = Event::ToolResult(ToolResultEvent::new(
            "1",
            "bash",
            serde_json::json!({"command": "echo secret"}),
            vec![bb_core::types::ContentBlock::Text {
                text: "secret output".into(),
            }],
            None,
            false,
        ));
        let result = bus.emit(&event).await.unwrap();
        assert!(result.content.is_some());
        let content = result.content.unwrap();
        assert_eq!(content.len(), 1);
        assert!(content[0].to_string().contains("redacted"));
    }
}
