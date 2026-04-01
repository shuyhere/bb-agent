use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tracing::{debug, error, info, warn};



/// Embedded host.js runtime shipped with the binary.
const HOST_JS: &str = include_str!("../js/host.js");

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

/// A running plugin host process that loads and executes TS plugins.
pub struct PluginHost {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout_reader: BufReader<tokio::process::ChildStdout>,
    next_id: AtomicU64,
    registered_tools: Vec<RegisteredTool>,
    registered_commands: Vec<RegisteredCommand>,
    plugin_count: usize,
}

impl PluginHost {
    /// Load plugins by spawning a Node.js process with the embedded host.js runtime.
    ///
    /// Writes host.js to a temp file, spawns Node with plugin paths as arguments,
    /// then reads startup notifications (tool_registered, command_registered, plugins_loaded).
    pub async fn load_plugins(plugin_paths: &[PathBuf]) -> Result<Self, PluginHostError> {
        if plugin_paths.is_empty() {
            return Err(PluginHostError::NoPlugins);
        }

        // Write host.js to a temp file
        let temp_dir = std::env::temp_dir().join("bb-agent-plugin-host");
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| PluginHostError::Io(format!("create temp dir: {e}")))?;
        let host_js_path = temp_dir.join("host.js");
        std::fs::write(&host_js_path, HOST_JS)
            .map_err(|e| PluginHostError::Io(format!("write host.js: {e}")))?;

        // Build args: host.js + plugin paths
        let mut args: Vec<String> = vec![host_js_path.to_string_lossy().to_string()];
        for p in plugin_paths {
            args.push(p.to_string_lossy().to_string());
        }

        let mut child = Command::new("node")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| PluginHostError::SpawnFailed(format!("node: {e}")))?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");

        info!("Plugin host spawned (pid: {:?}), loading {} plugin(s)", child.id(), plugin_paths.len());

        let mut host = Self {
            child,
            stdin,
            stdout_reader: BufReader::new(stdout),
            next_id: AtomicU64::new(1),
            registered_tools: Vec::new(),
            registered_commands: Vec::new(),
            plugin_count: 0,
        };

        // Read startup notifications until plugins_loaded
        host.read_startup_notifications().await?;

        info!(
            "Plugin host ready: {} plugin(s), {} tool(s), {} command(s)",
            host.plugin_count,
            host.registered_tools.len(),
            host.registered_commands.len(),
        );

        Ok(host)
    }

    /// Read notifications emitted during plugin loading (tool_registered, command_registered,
    /// plugin_error, plugins_loaded). Returns when plugins_loaded is received or timeout.
    async fn read_startup_notifications(&mut self) -> Result<(), PluginHostError> {
        let timeout = Duration::from_secs(10);

        loop {
            let msg = tokio::time::timeout(timeout, self.read_message()).await
                .map_err(|_| PluginHostError::Timeout("startup notifications".into()))?
                .map_err(|e| PluginHostError::Io(format!("read startup: {e}")))?;

            let msg = match msg {
                Some(m) => m,
                None => return Err(PluginHostError::ProcessExited),
            };

            // Startup messages are notifications (no id), with a method field
            let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
            let params = msg.get("params").cloned().unwrap_or(serde_json::json!({}));

            match method {
                "tool_registered" => {
                    if let Ok(tool) = serde_json::from_value::<RegisteredTool>(params) {
                        info!("Plugin registered tool: {}", tool.name);
                        self.registered_tools.push(tool);
                    }
                }
                "command_registered" => {
                    if let Ok(cmd) = serde_json::from_value::<RegisteredCommand>(params) {
                        info!("Plugin registered command: {}", cmd.name);
                        self.registered_commands.push(cmd);
                    }
                }
                "plugin_error" => {
                    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("?");
                    let error = params.get("error").and_then(|v| v.as_str()).unwrap_or("?");
                    error!("Plugin load error [{}]: {}", path, error);
                }
                "plugins_loaded" => {
                    self.plugin_count = params.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    return Ok(());
                }
                other => {
                    debug!("Unknown startup notification: {}", other);
                }
            }
        }
    }

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

    /// Get the list of tools registered by plugins.
    pub fn registered_tools(&self) -> &[RegisteredTool] {
        &self.registered_tools
    }

    /// Get the list of commands registered by plugins.
    pub fn registered_commands(&self) -> &[RegisteredCommand] {
        &self.registered_commands
    }

    /// Get the number of loaded plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugin_count
    }

    /// Kill the plugin host process.
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }

    // ── Internal helpers ─────────────────────────────────────────────

    async fn send_json(&mut self, value: &serde_json::Value) -> Result<(), std::io::Error> {
        let json = serde_json::to_string(value).unwrap();
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Read a single JSON message from stdout.
    async fn read_message(&mut self) -> Result<Option<serde_json::Value>, std::io::Error> {
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
    async fn read_response_for_id(&mut self, id: u64) -> Result<Option<serde_json::Value>, std::io::Error> {
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

impl Drop for PluginHost {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// Serialize an Event into a JSON value suitable for the plugin host.
fn serialize_event(event: &bb_hooks::Event) -> serde_json::Value {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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

    #[tokio::test]
    async fn test_load_plugins_with_sample() {
        // Skip if node is not available
        if std::process::Command::new("node").arg("--version").output().is_err() {
            eprintln!("Skipping test: node not available");
            return;
        }

        // Create a temp plugin
        let temp_dir = std::env::temp_dir().join("bb-test-plugins");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let plugin_path = temp_dir.join("test-plugin.js");
        std::fs::write(&plugin_path, r#"
            module.exports = function(bb) {
                bb.on("session_start", (event, ctx) => {
                    return { action: "started" };
                });

                bb.on("tool_call", (event, ctx) => {
                    if (event.tool_name === "bash" && event.input.command && event.input.command.includes("rm -rf /")) {
                        return { block: true, reason: "Blocked dangerous command" };
                    }
                });

                bb.registerTool({
                    name: "greet",
                    description: "Greet someone",
                    parameters: { type: "object", properties: { name: { type: "string" } } },
                    execute: async (toolCallId, params) => {
                        return { content: [{ type: "text", text: "Hello, " + (params.name || "world") + "!" }] };
                    },
                });
            };
        "#).unwrap();

        let mut host = PluginHost::load_plugins(&[plugin_path.clone()]).await.unwrap();

        // Verify plugin loaded
        assert_eq!(host.plugin_count(), 1);
        assert_eq!(host.registered_tools().len(), 1);
        assert_eq!(host.registered_tools()[0].name, "greet");

        // Test sending session_start event
        let result = host.send_event(&bb_hooks::Event::SessionStart).await;
        assert!(result.is_some());
        let hr = result.unwrap();
        assert_eq!(hr.action, Some("started".into()));

        // Test tool_call blocking
        let result = host.send_event(&bb_hooks::Event::ToolCall(bb_hooks::ToolCallEvent {
            tool_call_id: "tc1".into(),
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "rm -rf /"}),
        })).await;
        assert!(result.is_some());
        let hr = result.unwrap();
        assert_eq!(hr.block, Some(true));
        assert_eq!(hr.reason, Some("Blocked dangerous command".into()));

        // Test tool_call not blocking
        let result = host.send_event(&bb_hooks::Event::ToolCall(bb_hooks::ToolCallEvent {
            tool_call_id: "tc2".into(),
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
        })).await;
        // Should be None (no result from handler)
        assert!(result.is_none());

        // Test execute_tool
        let result = host.execute_tool("greet", "call1", serde_json::json!({"name": "Alice"})).await.unwrap();
        assert_eq!(result["content"][0]["text"], "Hello, Alice!");

        // Cleanup
        host.kill().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
