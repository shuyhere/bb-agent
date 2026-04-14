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
mod plugin_runtime;
mod runtime_support;
mod ui;

const EXTENSION_EVENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const EXTENSION_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(crate) use command_results::{ExtensionCommandOutcome, ExtensionMenuItem, ExtensionPromptSpec};
use command_results::{
    parse_command_activate_agent_result, parse_command_dispatch_result, parse_command_invocation,
    parse_command_menu_result, parse_command_prompt_result, render_command_result,
};
use discovery::discover_runtime_resources;
#[cfg(test)]
use discovery::{discover_package_resources, filter_matches, normalize_path, parse_frontmatter};
use packages::resolve_package_directories;
#[cfg(test)]
use packages::{
    PackageSource, classify_package_source, npm_package_name, package_install_root,
    resolve_package_directory,
};
pub(crate) use packages::{
    SettingsScope, auto_install_missing_packages, install_package, list_packages, remove_package,
    update_packages,
};
pub(crate) use runtime_support::{
    ExtensionBootstrap, RuntimeExtensionSupport, build_skill_system_prompt_section,
    load_runtime_extension_support, load_runtime_extension_support_with_ui,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputHookOutcome {
    pub handled: bool,
    pub text: Option<String>,
    pub output: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InputHookAction {
    #[default]
    Continue,
    Handled,
}

impl InputHookAction {
    fn from_hook_action(action: Option<&str>) -> Self {
        match action {
            Some("handled") => Self::Handled,
            _ => Self::Continue,
        }
    }
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

        let action = InputHookAction::from_hook_action(result.action.as_deref());
        let bb_hooks::HookResult {
            text: hook_text,
            message,
            ..
        } = result;

        Ok(match action {
            InputHookAction::Handled => InputHookOutcome {
                handled: true,
                text: None,
                output: hook_text.or_else(|| message.as_ref().and_then(render_command_result)),
            },
            InputHookAction::Continue => InputHookOutcome {
                handled: false,
                text: Some(hook_text.unwrap_or_else(|| text.to_string())),
                output: None,
            },
        })
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

#[cfg(test)]
mod tests;
