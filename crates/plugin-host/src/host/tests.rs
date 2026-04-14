use super::*;

#[tokio::test]
async fn test_load_plugins_with_sample() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: node not available");
        return;
    }

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

    let mut host = PluginHost::load_plugins(std::slice::from_ref(&plugin_path))
        .await
        .unwrap();

    assert_eq!(host.plugin_count(), 1);
    assert_eq!(host.registered_tools().len(), 1);
    assert_eq!(host.registered_tools()[0].name(), "greet");
    assert_eq!(host.registered_commands().len(), 1);
    assert_eq!(host.registered_commands()[0].name(), "hello");

    let result = host.send_event(&bb_hooks::Event::SessionStart).await;
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.action, Some("started".into()));

    let result = host
        .send_event(&bb_hooks::Event::ToolCall(bb_hooks::ToolCallEvent::new(
            "tc1",
            "bash",
            serde_json::json!({"command": "rm -rf /"}),
        )))
        .await;
    assert!(result.is_some());
    let hr = result.unwrap();
    assert_eq!(hr.block, Some(true));
    assert_eq!(hr.reason, Some("Blocked dangerous command".into()));

    let result = host
        .send_event(&bb_hooks::Event::ToolCall(bb_hooks::ToolCallEvent::new(
            "tc2",
            "bash",
            serde_json::json!({"command": "ls"}),
        )))
        .await;
    assert!(result.is_none());

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
                labels: std::collections::BTreeMap::from([(
                    "root".to_string(),
                    "top".to_string(),
                )]),
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
                        ctx.ui.notify('hello from extension', 'info');
                        ctx.ui.setStatus('my-ext', 'running...');
                        ctx.ui.setWidget('my-widget', ['line1', 'line2']);

                        const confirmed = await ctx.ui.confirm('Danger!', 'Continue?');
                        const selected = await ctx.ui.select('Pick', ['A', 'B', 'C']);
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

    let ui_handler: types::SharedUiHandler = std::sync::Arc::new(types::DefaultUiHandler);

    let mut host = PluginHost::load_plugins(std::slice::from_ref(&plugin_path))
        .await
        .unwrap();
    host.set_ui_handler(ui_handler);

    assert_eq!(host.registered_commands().len(), 1);
    assert_eq!(host.registered_commands()[0].name(), "ui-test");

    let result = host.execute_command("ui-test", "").await.unwrap();
    let message = result["message"].as_str().unwrap();
    assert_eq!(
        message,
        "confirmed=false selected=undefined typed=undefined"
    );

    let result = host.send_event(&bb_hooks::Event::SessionStart).await;
    assert!(result.is_none());

    host.kill().await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_load_plugins_rejects_empty_plugin_list() {
    let err = PluginHost::load_plugins(&[])
        .await
        .expect_err("empty plugin list should fail");
    assert!(matches!(err, types::PluginHostError::NoPlugins));
}

#[tokio::test]
async fn test_startup_ignores_invalid_json_lines_before_valid_registration() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: node not available");
        return;
    }

    let temp_dir = std::env::temp_dir().join("bb-test-invalid-startup-lines");
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");
    let plugin_path = temp_dir.join("startup-invalid.js");
    std::fs::write(
        &plugin_path,
        r#"
            process.stdout.write('not-json\n');
            module.exports = function(bb) {
                bb.registerCommand('still-loads', {
                    description: 'Load despite junk stdout',
                    handler: async () => ({ message: 'ok' }),
                });
            };
        "#,
    )
    .expect("write plugin");

    let mut host = PluginHost::load_plugins(std::slice::from_ref(&plugin_path))
        .await
        .expect("host should load despite junk lines");

    assert_eq!(host.plugin_count(), 1);
    assert_eq!(host.registered_commands().len(), 1);
    assert_eq!(host.registered_commands()[0].name(), "still-loads");

    host.kill().await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_execute_command_ignores_invalid_stdout_notifications() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: node not available");
        return;
    }

    let temp_dir = std::env::temp_dir().join("bb-test-invalid-runtime-lines");
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");
    let plugin_path = temp_dir.join("runtime-invalid.js");
    std::fs::write(
        &plugin_path,
        r#"
            module.exports = function(bb) {
                bb.registerCommand('runtime-junk', {
                    description: 'Emit junk stdout before responding',
                    handler: async () => {
                        process.stdout.write('not-json\n');
                        process.stdout.write(JSON.stringify({
                            jsonrpc: '2.0',
                            method: 'command_registered',
                            params: { description: 'missing name' }
                        }) + '\n');
                        return { message: 'ok' };
                    },
                });
            };
        "#,
    )
    .expect("write plugin");

    let mut host = PluginHost::load_plugins(std::slice::from_ref(&plugin_path))
        .await
        .expect("host should load");

    let result = host
        .execute_command("runtime-junk", "")
        .await
        .expect("command should succeed despite junk output");
    assert_eq!(result["message"], "ok");
    assert_eq!(host.registered_commands().len(), 1);

    host.kill().await;
    let _ = std::fs::remove_dir_all(&temp_dir);
}
