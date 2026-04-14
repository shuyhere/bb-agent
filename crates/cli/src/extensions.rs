use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use bb_core::agent_session_extensions::{
    ExtensionsResult, LoadedExtension, RegisteredCommand, RegisteredTool, SessionResourceBootstrap,
    SourceInfo, ToolDefinition,
};
use bb_core::error::{BbError, BbResult};
use bb_core::settings::Settings;
use bb_core::types::{ContentBlock, SessionEntry};
use bb_plugin_host::{
    PluginContext, PluginHost, RegisteredCommand as HostRegisteredCommand,
    RegisteredTool as HostRegisteredTool, SharedUiHandler,
};
use bb_tools::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::warn;

mod command_results;
mod discovery;
mod packages;
mod ui;

const EXTENSION_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const EXTENSION_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) use command_results::{ExtensionCommandOutcome, ExtensionMenuItem, ExtensionPromptSpec};
use command_results::{
    parse_command_activate_agent_result, parse_command_dispatch_result, parse_command_invocation,
    parse_command_menu_result, parse_command_prompt_result, render_command_result,
};
#[cfg(test)]
use discovery::{discover_package_resources, filter_matches, normalize_path, parse_frontmatter};
use discovery::{discover_runtime_resources, resolve_input_path};
#[cfg(test)]
use packages::{
    PackageSource, classify_package_source, npm_package_name, package_install_root,
    resolve_package_directory,
};
pub(crate) use packages::{
    SettingsScope, auto_install_missing_packages, install_package, list_packages, remove_package,
    update_packages,
};
use packages::{is_package_source, resolve_package_directories};
use ui::{ExtensionUiHandler, PrintUiHandler};

#[derive(Debug, Clone, Default)]
pub(crate) struct ExtensionBootstrap {
    pub paths: Vec<PathBuf>,
    pub package_sources: Vec<String>,
}

impl ExtensionBootstrap {
    pub(crate) fn from_cli_values(cwd: &Path, values: &[String]) -> Self {
        let mut bootstrap = Self::default();
        for value in values {
            if is_package_source(value) {
                bootstrap.package_sources.push(value.clone());
            } else {
                bootstrap.paths.push(resolve_input_path(cwd, value));
            }
        }
        bootstrap
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputHookOutcome {
    pub handled: bool,
    pub text: Option<String>,
    pub output: Option<String>,
}

#[derive(Clone)]
struct SessionSnapshotSource {
    conn: Arc<Mutex<rusqlite::Connection>>,
    session_id: String,
    session_file: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct ExtensionCommandRegistry {
    host: Option<Arc<Mutex<PluginHost>>>,
    commands: BTreeSet<String>,
    context: PluginContext,
    session: Option<SessionSnapshotSource>,
    pub(crate) ui_handler: Option<SharedUiHandler>,
}

impl fmt::Debug for ExtensionCommandRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionCommandRegistry")
            .field("commands", &self.commands)
            .field("context", &self.context)
            .field("has_host", &self.host.is_some())
            .field("has_session", &self.session.is_some())
            .field("has_ui_handler", &self.ui_handler.is_some())
            .finish_non_exhaustive()
    }
}

impl ExtensionCommandRegistry {
    pub(crate) fn bind_session_context(
        &mut self,
        conn: Arc<Mutex<rusqlite::Connection>>,
        session_id: impl Into<String>,
        session_file: Option<String>,
    ) {
        self.session = Some(SessionSnapshotSource {
            conn,
            session_id: session_id.into(),
            session_file,
        });
    }

    async fn build_context(&self) -> PluginContext {
        let mut context = self.context.clone();
        let Some(session) = &self.session else {
            return context;
        };

        let conn = session.conn.lock().await;
        let entries =
            bb_session::store::get_entries(&conn, &session.session_id).unwrap_or_default();
        let branch = bb_session::tree::active_path(&conn, &session.session_id).unwrap_or_default();
        let session_row = bb_session::store::get_session(&conn, &session.session_id)
            .ok()
            .flatten();
        drop(conn);

        context.session_entries = entries
            .into_iter()
            .filter_map(|row| bb_session::store::parse_entry(&row).ok())
            .filter_map(|entry| serde_json::to_value(entry).ok())
            .collect();
        context.session_branch = branch
            .into_iter()
            .filter_map(|row| bb_session::store::parse_entry(&row).ok())
            .filter_map(|entry| serde_json::to_value(entry).ok())
            .collect();
        context.leaf_id = session_row.as_ref().and_then(|row| row.leaf_id.clone());
        context.session_name = session_row.as_ref().and_then(|row| row.name.clone());
        context.session_file = session.session_file.clone();
        context.session_id = Some(session.session_id.clone());
        context.labels = build_labels_map(&context.session_entries);
        context
    }

    pub(crate) fn is_registered(&self, text: &str) -> bool {
        parse_command_invocation(text)
            .map(|(name, _)| self.commands.contains(name))
            .unwrap_or(false)
    }

    pub(crate) async fn execute_text(&self, text: &str) -> Result<Option<String>> {
        Ok(self.execute_text_structured(text).await?.into_text())
    }

    pub(crate) async fn execute_text_structured(
        &self,
        text: &str,
    ) -> Result<ExtensionCommandOutcome> {
        let Some((name, args)) = parse_command_invocation(text) else {
            return Ok(ExtensionCommandOutcome::Nothing);
        };
        if !self.commands.contains(name) {
            return Ok(ExtensionCommandOutcome::Nothing);
        }

        let Some(host) = &self.host else {
            bail!("extension command runtime is not available");
        };

        let result = match tokio::time::timeout(EXTENSION_COMMAND_TIMEOUT, async {
            let context = self.build_context().await;
            let mut host = host.lock().await;
            host.execute_command_with_context(name, args.unwrap_or_default(), &context)
                .await
        })
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => bail!("extension command failed: {err}"),
            Err(_) => bail!(
                "extension command timed out after {}s",
                EXTENSION_COMMAND_TIMEOUT.as_secs()
            ),
        };

        if let Some(menu) = parse_command_menu_result(name, &result) {
            return Ok(menu);
        }
        if let Some(prompt) = parse_command_prompt_result(name, &result) {
            return Ok(prompt);
        }
        if let Some(dispatch) = parse_command_dispatch_result(&result) {
            return Ok(dispatch);
        }
        if let Some(activate) = parse_command_activate_agent_result(&result) {
            return Ok(activate);
        }
        match render_command_result(&result) {
            Some(text) => Ok(ExtensionCommandOutcome::Text(text)),
            None => Ok(ExtensionCommandOutcome::Nothing),
        }
    }

    pub(crate) async fn send_event(&self, event: &bb_hooks::Event) -> Option<bb_hooks::HookResult> {
        let host = self.host.as_ref()?;

        match tokio::time::timeout(EXTENSION_EVENT_TIMEOUT, async {
            let context = self.build_context().await;
            let mut host = host.lock().await;
            host.send_event_with_context(event, &context).await
        })
        .await
        {
            Ok(result) => result,
            Err(_) => {
                warn!(
                    "extension event timed out after {}s: {:?}",
                    EXTENSION_EVENT_TIMEOUT.as_secs(),
                    event
                );
                None
            }
        }
    }

    pub(crate) async fn apply_input_hooks(
        &self,
        text: &str,
        source: &str,
    ) -> Result<InputHookOutcome> {
        let Some(result) = self
            .send_event(&bb_hooks::Event::Input(bb_hooks::events::InputEvent {
                text: text.to_string(),
                source: source.to_string(),
            }))
            .await
        else {
            return Ok(InputHookOutcome {
                handled: false,
                text: Some(text.to_string()),
                output: None,
            });
        };

        let action = result.action.as_deref().unwrap_or("continue");
        let transformed_text = result.text.clone().or_else(|| Some(text.to_string()));
        let output = if action == "handled" {
            result
                .text
                .clone()
                .or_else(|| result.message.as_ref().and_then(render_command_result))
        } else {
            None
        };

        Ok(InputHookOutcome {
            handled: action == "handled",
            text: if action == "handled" {
                None
            } else {
                transformed_text
            },
            output,
        })
    }
}

#[derive(Default)]
pub(crate) struct RuntimeExtensionSupport {
    pub session_resources: SessionResourceBootstrap,
    pub tools: Vec<Box<dyn Tool>>,
    pub commands: ExtensionCommandRegistry,
}

pub(crate) fn build_skill_system_prompt_section(resources: &SessionResourceBootstrap) -> String {
    let mut sections = Vec::new();

    if !resources.skills.is_empty() {
        let mut skill_lines = Vec::new();
        skill_lines.push("<available_skills>".to_string());
        for skill in &resources.skills {
            skill_lines.push("  <skill>".to_string());
            skill_lines.push(format!("    <name>{}</name>", skill.info.name));
            skill_lines.push(format!(
                "    <description>{}</description>",
                skill.info.description
            ));
            skill_lines.push(format!(
                "    <location>{}</location>",
                skill.info.source_info.path
            ));
            skill_lines.push("  </skill>".to_string());
        }
        skill_lines.push("</available_skills>".to_string());
        sections.push(skill_lines.join("\n"));
    }

    if !resources.prompts.is_empty() {
        let prompt_list: Vec<String> = resources
            .prompts
            .iter()
            .map(|prompt| format!("- /{}: {}", prompt.info.name, prompt.info.description))
            .collect();
        sections.push(format!(
            "Available prompt templates (invoke with /name):\n{}",
            prompt_list.join("\n")
        ));
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", sections.join("\n\n"))
    }
}

pub(crate) async fn load_runtime_extension_support(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
) -> Result<RuntimeExtensionSupport> {
    load_runtime_extension_support_with_ui(cwd, settings, bootstrap, false).await
}

pub(crate) async fn load_runtime_extension_support_with_ui(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
    has_ui: bool,
) -> Result<RuntimeExtensionSupport> {
    let package_dirs = resolve_package_directories(cwd, settings, bootstrap)?;
    let discovered = discover_runtime_resources(cwd, settings, bootstrap, &package_dirs)?;

    let mut session_resources = SessionResourceBootstrap {
        skills: if settings.enable_skill_commands {
            discovered.skills
        } else {
            Vec::new()
        },
        prompts: discovered.prompts,
        ..SessionResourceBootstrap::default()
    };

    let (tools, commands, extensions) =
        build_plugin_runtime(cwd, has_ui, &discovered.extension_files).await?;
    session_resources.extensions = extensions;

    Ok(RuntimeExtensionSupport {
        session_resources,
        tools,
        commands,
    })
}

async fn build_plugin_runtime(
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
            .map(|command| command.name.clone())
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

fn build_labels_map(entries: &[Value]) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    for entry in entries {
        if let Ok(SessionEntry::Label {
            target_id, label, ..
        }) = serde_json::from_value::<SessionEntry>(entry.clone())
        {
            match label {
                Some(label) => {
                    labels.insert(target_id.to_string(), label);
                }
                None => {
                    labels.remove(&target_id.to_string());
                }
            }
        }
    }
    labels
}

fn map_plugin_command_registration(command: &HostRegisteredCommand) -> RegisteredCommand {
    RegisteredCommand {
        invocation_name: command.name.clone(),
        description: command.description.clone(),
        source_info: SourceInfo {
            path: format!("<command:{}>", command.name),
            source: "extension:plugin-host".to_string(),
        },
    }
}

fn map_plugin_tool_registration(tool: &HostRegisteredTool) -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: tool.name.clone(),
            prompt_snippet: None,
            prompt_guidelines: Vec::new(),
        },
        source_info: SourceInfo {
            path: format!("<tool:{}>", tool.name),
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
        &self.registration.name
    }

    fn description(&self) -> &str {
        &self.registration.description
    }

    fn parameters_schema(&self) -> Value {
        self.registration.parameters.clone()
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

fn map_tool_result(value: Value) -> BbResult<ToolResult> {
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

#[cfg(test)]
mod tests;
