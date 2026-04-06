mod serialize;
mod ui;

use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tracing::{debug, info, warn};

use super::PluginHost;
use super::types::{PluginContext, PluginHostError, RegisteredCommand, RegisteredTool};

use serialize::serialize_event;

impl PluginHost {
    /// Send an event to plugins and await the merged result.
    ///
    /// Serializes the event as a JSON-RPC request with method "event",
    /// waits for the response with the matching id.
    pub async fn send_event(&mut self, event: &bb_hooks::Event) -> Option<bb_hooks::HookResult> {
        self.send_event_with_context(event, &PluginContext::default())
            .await
    }

    pub async fn send_event_with_context(
        &mut self,
        event: &bb_hooks::Event,
        context: &PluginContext,
    ) -> Option<bb_hooks::HookResult> {
        let event_data = serialize_event(event);
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "event",
            "params": {
                "event": event_data,
                "context": context,
            },
        });

        if let Err(e) = self.send_json(&request).await {
            warn!("Failed to send event to plugin host: {e}");
            return None;
        }

        match tokio::time::timeout(Duration::from_secs(30), self.read_response_for_id(id)).await {
            Ok(Ok(Some(result))) => match serde_json::from_value::<bb_hooks::HookResult>(result) {
                Ok(hr) => {
                    if matches!(event, bb_hooks::Event::ToolCall(tool_call)
                        if hr.block.is_none()
                            && hr.cancel.is_none()
                            && hr.reason.is_none()
                            && hr.messages.is_none()
                            && hr.system_prompt.is_none()
                            && hr.message.is_none()
                            && hr.content.is_none()
                            && hr.details.is_none()
                            && hr.is_error.is_none()
                            && hr.action.is_none()
                            && hr.text.is_none()
                            && hr.payload.is_none()
                            && hr.input.as_ref() == Some(&tool_call.input))
                    {
                        return None;
                    }

                    if hr.block.is_some()
                        || hr.cancel.is_some()
                        || hr.reason.is_some()
                        || hr.messages.is_some()
                        || hr.system_prompt.is_some()
                        || hr.message.is_some()
                        || hr.content.is_some()
                        || hr.details.is_some()
                        || hr.is_error.is_some()
                        || hr.input.is_some()
                        || hr.action.is_some()
                        || hr.text.is_some()
                        || hr.payload.is_some()
                    {
                        Some(hr)
                    } else {
                        None
                    }
                }
                Err(_) => None,
            },
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

        self.send_json(&request)
            .await
            .map_err(|e| PluginHostError::Io(format!("send execute_tool: {e}")))?;

        match tokio::time::timeout(Duration::from_secs(60), self.read_response_for_id(id)).await {
            Ok(Ok(Some(result))) => Ok(result),
            Ok(Ok(None)) => Err(PluginHostError::ProcessExited),
            Ok(Err(e)) => Err(PluginHostError::Io(format!("read tool response: {e}"))),
            Err(_) => Err(PluginHostError::Timeout(format!("execute_tool {name}"))),
        }
    }

    /// Execute a plugin-registered command.
    pub async fn execute_command(
        &mut self,
        name: &str,
        args: &str,
    ) -> Result<serde_json::Value, PluginHostError> {
        self.execute_command_with_context(name, args, &PluginContext::default())
            .await
    }

    pub async fn execute_command_with_context(
        &mut self,
        name: &str,
        args: &str,
        context: &PluginContext,
    ) -> Result<serde_json::Value, PluginHostError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "execute_command",
            "params": {
                "name": name,
                "args": args,
                "context": context,
            },
        });

        self.send_json(&request)
            .await
            .map_err(|e| PluginHostError::Io(format!("send execute_command: {e}")))?;

        match tokio::time::timeout(Duration::from_secs(60), self.read_response_for_id(id)).await {
            Ok(Ok(Some(result))) => Ok(result),
            Ok(Ok(None)) => Err(PluginHostError::ProcessExited),
            Ok(Err(e)) => Err(PluginHostError::Io(format!("read command response: {e}"))),
            Err(_) => Err(PluginHostError::Timeout(format!("execute_command {name}"))),
        }
    }

    pub(super) async fn send_json(
        &mut self,
        value: &serde_json::Value,
    ) -> Result<(), std::io::Error> {
        let json = serde_json::to_string(value).map_err(std::io::Error::other)?;
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    pub(super) async fn read_message(
        &mut self,
    ) -> Result<Option<serde_json::Value>, std::io::Error> {
        let mut line = String::new();
        let bytes = self.stdout_reader.read_line(&mut line).await?;
        if bytes == 0 {
            return Ok(None);
        }
        match serde_json::from_str(&line) {
            Ok(val) => Ok(Some(val)),
            Err(e) => {
                warn!(
                    "Failed to parse plugin message: {e} — line: {}",
                    line.trim()
                );
                Ok(None)
            }
        }
    }

    pub(super) async fn read_response_for_id(
        &mut self,
        id: u64,
    ) -> Result<Option<serde_json::Value>, std::io::Error> {
        loop {
            let msg = match self.read_message().await? {
                Some(m) => m,
                None => return Ok(None),
            };

            if let Some(msg_id) = msg.get("id")
                && msg_id.as_u64() == Some(id)
            {
                if let Some(err) = msg.get("error") {
                    warn!("Plugin host returned error: {:?}", err);
                    return Ok(None);
                }
                return Ok(msg.get("result").cloned());
            }

            let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
            match method {
                "handler_error" => {
                    let params = msg.get("params").cloned().unwrap_or_default();
                    let event_type = params
                        .get("event_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?");
                    let error = params.get("error").and_then(|v| v.as_str()).unwrap_or("?");
                    warn!("Plugin handler error [{}]: {}", event_type, error);
                }
                "tool_registered" => {
                    if let Some(params) = msg.get("params")
                        && let Ok(tool) = serde_json::from_value::<RegisteredTool>(params.clone())
                    {
                        info!("Plugin registered tool (late): {}", tool.name);
                        self.registered_tools.push(tool);
                    }
                }
                "command_registered" => {
                    if let Some(params) = msg.get("params")
                        && let Ok(cmd) = serde_json::from_value::<RegisteredCommand>(params.clone())
                    {
                        info!("Plugin registered command (late): {}", cmd.name);
                        self.registered_commands.push(cmd);
                    }
                }
                "ui_request" => {
                    if let Some(params) = msg.get("params") {
                        self.handle_ui_request_inline(params.clone()).await;
                    }
                }
                _ => {
                    debug!("Ignoring notification during response wait: {}", method);
                }
            }
        }
    }
}
