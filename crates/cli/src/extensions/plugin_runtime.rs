use super::ui::{ExtensionUiHandler, PrintUiHandler};
use super::*;

pub(super) async fn build_plugin_runtime(
    cwd: &Path,
    has_ui: bool,
    extension_files: &[PathBuf],
) -> Result<(
    Vec<Box<dyn Tool>>,
    ExtensionCommandRegistry,
    ExtensionsResult,
)> {
    if extension_files.is_empty() {
        return Ok((
            Vec::new(),
            ExtensionCommandRegistry::default(),
            ExtensionsResult::default(),
        ));
    }

    let ui_handler = make_ui_handler(has_ui);
    let mut host = PluginHost::load_plugins(extension_files).await?;
    host.set_ui_handler(ui_handler.clone());
    let tool_registrations = host.registered_tools().to_vec();
    let command_registrations = host.registered_commands().to_vec();
    let shared_host = Arc::new(Mutex::new(host));

    let tools = tool_registrations
        .iter()
        .cloned()
        .map(|registration| {
            Box::new(PluginTool::new(shared_host.clone(), registration)) as Box<dyn Tool>
        })
        .collect();

    let commands = ExtensionCommandRegistry {
        host: Some(shared_host),
        commands: command_registrations
            .iter()
            .map(|command| command.name().to_string())
            .collect(),
        context: PluginContext {
            cwd: Some(cwd.display().to_string()),
            has_ui,
            ..PluginContext::default()
        },
        session: None,
        ui_handler: Some(ui_handler),
    };

    let extensions = ExtensionsResult {
        extensions: extension_files
            .iter()
            .map(|path| LoadedExtension {
                path: path.display().to_string(),
            })
            .collect(),
        registered_commands: command_registrations
            .iter()
            .map(map_plugin_command_registration)
            .collect(),
        registered_tools: tool_registrations
            .iter()
            .map(map_plugin_tool_registration)
            .collect(),
        ..ExtensionsResult::default()
    };

    Ok((tools, commands, extensions))
}

fn make_ui_handler(has_ui: bool) -> SharedUiHandler {
    if has_ui {
        Arc::new(ExtensionUiHandler::default())
    } else {
        Arc::new(PrintUiHandler)
    }
}

fn map_plugin_command_registration(command: &HostRegisteredCommand) -> RegisteredCommand {
    RegisteredCommand {
        invocation_name: command.name().to_string(),
        description: command.description().to_string(),
        source_info: SourceInfo {
            path: format!("<command:{}>", command.name()),
            source: "extension:plugin-host".to_string(),
        },
    }
}

fn map_plugin_tool_registration(tool: &HostRegisteredTool) -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: tool.name().to_string(),
            prompt_snippet: None,
            prompt_guidelines: Vec::new(),
        },
        source_info: SourceInfo {
            path: format!("<tool:{}>", tool.name()),
            source: "extension:plugin-host".to_string(),
        },
    }
}

struct PluginTool {
    host: Arc<Mutex<PluginHost>>,
    registration: HostRegisteredTool,
}

impl PluginTool {
    fn new(host: Arc<Mutex<PluginHost>>, registration: HostRegisteredTool) -> Self {
        Self { host, registration }
    }
}

#[async_trait]
impl Tool for PluginTool {
    fn name(&self) -> &str {
        self.registration.name()
    }

    fn description(&self) -> &str {
        self.registration.description()
    }

    fn parameters_schema(&self) -> Value {
        self.registration.parameters().clone()
    }

    async fn execute(
        &self,
        params: Value,
        _ctx: &ToolContext,
        _cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let mut host = self.host.lock().await;
        let result = host
            .execute_tool(self.name(), self.name(), params)
            .await
            .map_err(|err| BbError::Plugin(err.to_string()))?;
        map_tool_result(result)
    }
}

pub(super) fn map_tool_result(value: Value) -> BbResult<ToolResult> {
    let mut content = Vec::new();

    if let Some(blocks) = value.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        content.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
                "image" => {
                    let data = block.get("data").and_then(Value::as_str);
                    let mime_type = block
                        .get("mime_type")
                        .or_else(|| block.get("mimeType"))
                        .and_then(Value::as_str);
                    if let (Some(data), Some(mime_type)) = (data, mime_type) {
                        content.push(ContentBlock::Image {
                            data: data.to_string(),
                            mime_type: mime_type.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    if content.is_empty() {
        if let Some(text) = value.get("text").and_then(Value::as_str) {
            content.push(ContentBlock::Text {
                text: text.to_string(),
            });
        } else {
            content.push(ContentBlock::Text {
                text: serde_json::to_string_pretty(&value).map_err(BbError::Json)?,
            });
        }
    }

    Ok(ToolResult {
        content,
        details: value.get("details").cloned(),
        is_error: value
            .get("isError")
            .or_else(|| value.get("is_error"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        artifact_path: None,
    })
}
