use super::*;

#[tokio::test]
async fn test_load_plugins_with_sample() {
    // Skip if node is not available
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: node not available");
        return;
    }

    // Create a temp plugin
    let temp_dir = std::env::temp_dir().join("bb-test-plugins");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let plugin_path = temp_dir.join("test-plugin.js");
    std::fs::write(
        &plugin_path,
        r#"
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

                bb.registerCommand("hello", {
                    description: "Say hello",
                    handler: async (args, ctx) => ({
                        message: "Hello command " + (args || "world")
                            + " @ " + ctx.cwd
                            + " ui=" + ctx.hasUI
                            + " entries=" + ctx.sessionManager.getEntries().length
                            + " leaf=" + ctx.sessionManager.getLeafId()
                            + " label=" + ctx.sessionManager.getLabel("root")
                    })
                });
            };
        "#,
    )
    .unwrap();

    let mut host = PluginHost::load_plugins(&[plugin_path.clone()])
        .await
        .unwrap();

    // Verify plugin loaded
    assert_eq!(host.plugin_count(), 1);
    assert_eq!(host.registered_tools().len(), 1);
    assert_eq!(host.registered_tools()[0].name, "greet");
    assert_eq!(host.registered_commands().len(), 1);
    assert_eq!(host.registered_commands()[0].name, "hello");

    // Test sending session_start event
    let result = host.send_event(&bb_hooks::Event::SessionStart).await;
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.action, Some("started".into()));

    // Test tool_call blocking
    let result = host
        .send_event(&bb_hooks::Event::ToolCall(bb_hooks::ToolCallEvent {
            tool_call_id: "tc1".into(),
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "rm -rf /"}),
        }))
        .await;
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.block, Some(true));
    assert_eq!(hr.reason, Some("Blocked dangerous command".into()));

    // Test tool_call not blocking
    let result = host
        .send_event(&bb_hooks::Event::ToolCall(bb_hooks::ToolCallEvent {
            tool_call_id: "tc2".into(),
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
        }))
        .await;
    assert!(result.is_none());

    // Test execute_tool
    let result = host
        .execute_tool("greet", "call1", serde_json::json!({"name": "Alice"}))
        .await
        .unwrap();
    assert_eq!(result["content"][0]["text"], "Hello, Alice!");

    let result = host
        .execute_command_with_context(
            "hello",
            "Alice",
            &PluginContext {
                cwd: Some("/tmp/plugin-test".to_string()),
                has_ui: true,
                session_entries: vec![serde_json::json!({
                    "type": "message",
                    "id": "root",
                    "parent_id": null,
                    "timestamp": "2026-01-01T00:00:00Z",
                    "message": {"role": "user", "content": [{"type": "text", "text": "hi"}], "timestamp": 0}
                })],
                session_branch: vec![serde_json::json!({
                    "type": "message",
                    "id": "root",
                    "parent_id": null,
                    "timestamp": "2026-01-01T00:00:00Z",
                    "message": {"role": "user", "content": [{"type": "text", "text": "hi"}], "timestamp": 0}
                })],
                leaf_id: Some("root".to_string()),
                labels: std::collections::BTreeMap::from([("root".to_string(), "top".to_string())]),
                session_file: None,
                session_id: Some("session-1".to_string()),
                session_name: Some("demo".to_string()),
                system_prompt: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        result["message"],
        "Hello command Alice @ /tmp/plugin-test ui=true entries=1 leaf=root label=top"
    );

    // Cleanup
    host.kill().await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_extension_ui_plumbing() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: node not available");
        return;
    }

    let temp_dir = std::env::temp_dir().join("bb-test-ui-ext");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let plugin_path = temp_dir.join("ui-ext.js");
    std::fs::write(
        &plugin_path,
        r#"
            module.exports = function(bb) {
                bb.registerCommand('ui-test', {
                    description: 'Test UI methods',
                    handler: async (args, ctx) => {
                        // Fire-and-forget: notify
                        ctx.ui.notify('hello from extension', 'info');
                        ctx.ui.setStatus('my-ext', 'running...');
                        ctx.ui.setWidget('my-widget', ['line1', 'line2']);

                        // Dialog: confirm (will get default response: false)
                        const confirmed = await ctx.ui.confirm('Danger!', 'Continue?');

                        // Dialog: select (will get default: cancelled)
                        const selected = await ctx.ui.select('Pick', ['A', 'B', 'C']);

                        // Dialog: input (will get default: cancelled)
                        const typed = await ctx.ui.input('Name?', 'enter name');

                        return {
                            message: `confirmed=${confirmed} selected=${selected} typed=${typed}`,
                        };
                    },
                });

                bb.on('session_start', async (_event, ctx) => {
                    ctx.ui.notify('session started!', 'info');
                    return {};
                });
            };
        "#,
    )
    .unwrap();

    // Use default UI handler (returns defaults for dialogs, logs notifications)
    let ui_handler: types::SharedUiHandler = std::sync::Arc::new(types::DefaultUiHandler);

    let mut host = PluginHost::load_plugins(&[plugin_path.clone()])
        .await
        .unwrap();
    host.set_ui_handler(ui_handler);

    assert_eq!(host.registered_commands().len(), 1);
    assert_eq!(host.registered_commands()[0].name, "ui-test");

    // Execute the command — it will call notify, setStatus, setWidget, confirm, select, input
    // All dialog methods should return defaults (false, cancelled, cancelled)
    let result = host.execute_command("ui-test", "").await.unwrap();
    let message = result["message"].as_str().unwrap();
    assert_eq!(
        message,
        "confirmed=false selected=undefined typed=undefined"
    );

    // Also test that notify in event handler works
    let result = host.send_event(&bb_hooks::Event::SessionStart).await;
    // The handler returns {} so result should be None (no meaningful content)
    // But the notify call should not cause a hang
    assert!(result.is_none());

    host.kill().await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}
