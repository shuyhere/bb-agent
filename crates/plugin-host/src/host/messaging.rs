use std::sync::atomic::Ordering;
use std::time::Duration;

use tracing::{debug, info, warn};

use super::PluginHost;
use super::types::{PluginHostError, RegisteredCommand, RegisteredTool};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

impl PluginHost {
    /// Send an event to plugins and await the merged result.
    ///
    /// Serializes the event as a JSON-RPC request with method "event",
    /// waits for the response with the matching id.
    pub async fn send_event(&mut self, event: &bb_hooks::Event) -> Option<bb_hooks::HookResult> {
        let event_data = serialize_event(event);
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "event",
            "params": event_data,
        });

        if let Err(e) = self.send_json(&request).await {
            warn!("Failed to send event to plugin host: {e}");
            return None;
        }

        // Read response with timeout
        match tokio::time::timeout(Duration::from_secs(30), self.read_response_for_id(id)).await {
            Ok(Ok(Some(result))) => {
                // Parse into HookResult
                match serde_json::from_value::<bb_hooks::HookResult>(result) {
                    Ok(hr) => {
                        // Only return Some if there's actual content
                        if hr.block.is_some() || hr.cancel.is_some() || hr.reason.is_some()
                            || hr.messages.is_some() || hr.system_prompt.is_some()
                            || hr.message.is_some() || hr.content.is_some()
                            || hr.action.is_some() || hr.text.is_some()
                            || hr.payload.is_some()
                        {
                            Some(hr)
                        } else {
                            None
                        }
                    }
                    Err(_) => None,
                }
            }
            Ok(Ok(None)) => None,
            Ok(Err(e)) => {
                warn!("Error reading event response: {e}");
                None
            }
            Err(_) => {
                warn!("Timeout waiting for event response");
                None
            }
        }
    }

    /// Execute a plugin-registered tool.
    pub async fn execute_tool(
        &mut self,
        name: &str,
        tool_call_id: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, PluginHostError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "execute_tool",
            "params": {
                "name": name,
                "toolCallId": tool_call_id,
                "params": params,
            },
        });

        self.send_json(&request).await
            .map_err(|e| PluginHostError::Io(format!("send execute_tool: {e}")))?;

        match tokio::time::timeout(Duration::from_secs(60), self.read_response_for_id(id)).await {
            Ok(Ok(Some(result))) => Ok(result),
            Ok(Ok(None)) => Err(PluginHostError::ProcessExited),
            Ok(Err(e)) => Err(PluginHostError::Io(format!("read tool response: {e}"))),
            Err(_) => Err(PluginHostError::Timeout(format!("execute_tool {name}"))),
        }
    }

    // ── Internal helpers ─────────────────────────────────────────────

    pub(super) async fn send_json(&mut self, value: &serde_json::Value) -> Result<(), std::io::Error> {
        let json = serde_json::to_string(value).unwrap();
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Read a single JSON message from stdout.
    pub(super) async fn read_message(&mut self) -> Result<Option<serde_json::Value>, std::io::Error> {
        let mut line = String::new();
        let bytes = self.stdout_reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(None);
        }
        match serde_json::from_str(&line) {
            Ok(val) => Ok(Some(val)),
            Err(e) => {
                warn!("Failed to parse plugin message: {e} — line: {}", line.trim());
                Ok(None)
            }
        }
    }

    /// Read messages until we find a response with the given id.
    /// Non-matching notifications are processed inline.
    pub(super) async fn read_response_for_id(&mut self, id: u64) -> Result<Option<serde_json::Value>, std::io::Error> {
        loop {
            let msg = match self.read_message().await? {
                Some(m) => m,
                None => return Ok(None),
            };

            // Check if this is our response
            if let Some(msg_id) = msg.get("id") {
                if msg_id.as_u64() == Some(id) {
                    // Check for error
                    if let Some(err) = msg.get("error") {
                        warn!("Plugin host returned error: {:?}", err);
                        return Ok(None);
                    }
                    return Ok(msg.get("result").cloned());
                }
            }

            // It's a notification — handle inline
            let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
            match method {
                "handler_error" => {
                    let params = msg.get("params").cloned().unwrap_or_default();
                    let event_type = params.get("event_type").and_then(|v| v.as_str()).unwrap_or("?");
                    let error = params.get("error").and_then(|v| v.as_str()).unwrap_or("?");
                    warn!("Plugin handler error [{}]: {}", event_type, error);
                }
                "tool_registered" => {
                    if let Some(params) = msg.get("params") {
                        if let Ok(tool) = serde_json::from_value::<RegisteredTool>(params.clone()) {
                            info!("Plugin registered tool (late): {}", tool.name);
                            self.registered_tools.push(tool);
                        }
                    }
                }
                "command_registered" => {
                    if let Some(params) = msg.get("params") {
                        if let Ok(cmd) = serde_json::from_value::<RegisteredCommand>(params.clone()) {
                            info!("Plugin registered command (late): {}", cmd.name);
                            self.registered_commands.push(cmd);
                        }
                    }
                }
                _ => {
                    debug!("Ignoring notification during response wait: {}", method);
                }
            }
        }
    }
}

/// Serialize an Event into a JSON value suitable for the plugin host.
pub(super) fn serialize_event(event: &bb_hooks::Event) -> serde_json::Value {
    use bb_hooks::Event;

    let event_type = event.event_type();
    let data = match event {
        Event::SessionStart => serde_json::json!({ "type": event_type }),
        Event::SessionShutdown => serde_json::json!({ "type": event_type }),
        Event::AgentEnd => serde_json::json!({ "type": event_type }),
        Event::TurnStart { turn_index } => serde_json::json!({ "type": event_type, "turn_index": turn_index }),
        Event::TurnEnd { turn_index } => serde_json::json!({ "type": event_type, "turn_index": turn_index }),
        Event::ToolCall(tc) => serde_json::json!({
            "type": event_type,
            "tool_call_id": tc.tool_call_id,
            "tool_name": tc.tool_name,
            "input": tc.input,
        }),
        Event::ToolResult(tr) => serde_json::json!({
            "type": event_type,
            "tool_call_id": tr.tool_call_id,
            "tool_name": tr.tool_name,
            "is_error": tr.is_error,
        }),
        Event::BeforeAgentStart { prompt, system_prompt } => serde_json::json!({
            "type": event_type,
            "prompt": prompt,
            "system_prompt": system_prompt,
        }),
        Event::SessionBeforeCompact(prep) => serde_json::json!({
            "type": event_type,
            "preparation": {
                "firstKeptEntryId": prep.first_kept_entry_id,
                "tokensBefore": prep.tokens_before,
            },
        }),
        Event::SessionCompact { from_plugin } => serde_json::json!({
            "type": event_type,
            "from_plugin": from_plugin,
        }),
        Event::SessionBeforeTree(prep) => serde_json::json!({
            "type": event_type,
            "target_id": prep.target_id,
            "old_leaf_id": prep.old_leaf_id,
        }),
        Event::SessionTree { new_leaf, old_leaf } => serde_json::json!({
            "type": event_type,
            "new_leaf": new_leaf,
            "old_leaf": old_leaf,
        }),
        Event::Context(ctx) => serde_json::json!({
            "type": event_type,
            "message_count": ctx.messages.len(),
        }),
        Event::BeforeProviderRequest { payload } => serde_json::json!({
            "type": event_type,
            "payload": payload,
        }),
        Event::Input(input) => serde_json::json!({
            "type": event_type,
            "text": input.text,
            "source": input.source,
        }),
    };

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_event_session_start() {
        let event = bb_hooks::Event::SessionStart;
        let json = serialize_event(&event);
        assert_eq!(json["type"], "session_start");
    }

    #[test]
    fn test_serialize_event_tool_call() {
        let event = bb_hooks::Event::ToolCall(bb_hooks::ToolCallEvent {
            tool_call_id: "tc1".into(),
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
        });
        let json = serialize_event(&event);
        assert_eq!(json["type"], "tool_call");
        assert_eq!(json["tool_name"], "bash");
        assert_eq!(json["input"]["command"], "ls");
    }
}
