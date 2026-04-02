mod types;
mod lifecycle;
mod messaging;

pub use types::{PluginHostError, RegisteredCommand, RegisteredTool};

use std::sync::atomic::AtomicU64;
use tokio::io::BufReader;
use tokio::process::Child;

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
