use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::SystemTime,
};

use bb_core::agent_session_runtime::AgentSessionRuntimeHost;
use bb_tools::{ToolApprovalOutcome, ToolApprovalRequest};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::compaction_exec::ExecutedCompaction;
use crate::session_bootstrap::{SessionRuntimeSetup, SessionUiOptions};

mod loop_impl;
mod resources;
mod ui;

/// An image file queued for attachment to the next prompt.
#[derive(Clone, Debug)]
pub(super) struct PendingImage {
    pub data: String,
    pub mime_type: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct ResourceWatchState {
    global_settings_mtime: Option<SystemTime>,
    project_settings_mtime: Option<SystemTime>,
}

impl ResourceWatchState {
    fn capture(cwd: &std::path::Path) -> Self {
        Self {
            global_settings_mtime: settings_mtime(
                &bb_core::config::global_dir().join("settings.json"),
            ),
            project_settings_mtime: settings_mtime(
                &bb_core::config::project_dir(cwd).join("settings.json"),
            ),
        }
    }
}

fn settings_mtime(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum SessionApprovalRule {
    Exact(String),
    Prefix(String),
}

impl SessionApprovalRule {
    pub(super) fn matches(&self, command: &str) -> bool {
        match self {
            Self::Exact(exact) => command.trim() == exact,
            Self::Prefix(prefix) => {
                let command = command.trim();
                command == prefix
                    || command
                        .strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
            }
        }
    }

    pub(super) fn display_scope(&self) -> String {
        match self {
            Self::Exact(command) => format!("`{command}`"),
            Self::Prefix(prefix) => format!("commands that start with `{prefix}`"),
        }
    }
}

pub(super) fn derive_session_approval_rule(command: &str) -> SessionApprovalRule {
    let trimmed = command.trim().to_string();
    if trimmed.is_empty() || trimmed.contains('\n') || contains_shell_meta(&trimmed) {
        return SessionApprovalRule::Exact(trimmed);
    }

    let tokens = trimmed
        .split_whitespace()
        .filter(|token| !looks_like_env_assignment(token))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return SessionApprovalRule::Exact(trimmed);
    }

    let first = tokens[0];
    let prefix_len = match first {
        "git" | "cargo" | "npm" | "pnpm" | "yarn" | "make" | "cmake" | "python" | "python3"
        | "pip" | "pip3" | "go" | "docker" | "kubectl" | "uv" => 2,
        _ => 1,
    };
    let safe_len = prefix_len.min(tokens.len());
    if safe_len == 0 {
        SessionApprovalRule::Exact(trimmed)
    } else {
        SessionApprovalRule::Prefix(tokens[..safe_len].join(" "))
    }
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !value.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn contains_shell_meta(line: &str) -> bool {
    line.contains("&&")
        || line.contains("||")
        || line.contains(';')
        || line.contains("|&")
        || line.contains(">")
        || line.contains("<")
        || line.contains("$(")
        || line.contains('`')
}

pub(super) enum QueuedPrompt {
    Visible(String),
    Hidden(String),
}

#[derive(Clone)]
pub(super) struct PendingModelAuthSelection {
    pub model: bb_provider::registry::Model,
    pub thinking_override: Option<bb_core::agent_session::ThinkingLevel>,
}

pub(super) struct TuiController {
    pub(super) runtime_host: AgentSessionRuntimeHost,
    pub(super) session_setup: SessionRuntimeSetup,
    pub(super) options: SessionUiOptions,
    pub(super) command_tx: mpsc::UnboundedSender<bb_tui::tui::TuiCommand>,
    pub(super) abort_token: CancellationToken,
    pub(super) streaming: bool,
    pub(super) retry_status: Option<String>,
    pub(super) queued_prompts: VecDeque<QueuedPrompt>,
    pub(super) pending_tree_summary_target: Option<String>,
    pub(super) pending_tree_custom_prompt_target: Option<String>,
    pub(super) pending_model_provider_search: Option<String>,
    pub(super) pending_model_auth_selection: Option<PendingModelAuthSelection>,
    pub(super) pending_login_api_key_provider: Option<String>,
    pub(super) pending_login_copilot_enterprise: bool,
    pub(super) pending_images: Vec<PendingImage>,
    pub(super) local_action_cancel: Option<CancellationToken>,
    pub(super) manual_compaction_in_progress: bool,
    pub(super) auto_compaction_in_progress: bool,
    pub(super) manual_compaction_generation: u64,
    pub(super) manual_compaction_tx: mpsc::UnboundedSender<ManualCompactionEvent>,
    pub(super) manual_compaction_rx: mpsc::UnboundedReceiver<ManualCompactionEvent>,
    pub(super) color_theme: bb_tui::tui::spinner::ColorTheme,
    pub(super) shutdown_requested: bool,
    pub(super) approval_rx: mpsc::UnboundedReceiver<PendingApprovalRequest>,
    pub(super) pending_approval: Option<PendingApprovalRequest>,
    pub(super) session_approval_rules: HashSet<SessionApprovalRule>,
    /// Menu IDs for `OpenSelectMenu` requests that came from an extension
    /// command → originating command name. Used so that when the user picks
    /// a value we can re-invoke `/<command> <value>`.
    pub(super) pending_extension_menus: HashMap<String, String>,
    /// Active auth-style input dialog owned by an extension command.
    pub(super) pending_extension_prompt: Option<crate::extensions::ExtensionPromptSpec>,
    resource_watch: ResourceWatchState,
    suppress_next_resource_watch_reload: bool,
}

pub(super) struct PendingApprovalRequest {
    pub request: ToolApprovalRequest,
    pub response_tx: oneshot::Sender<ToolApprovalOutcome>,
}

pub(super) enum ManualCompactionEvent {
    Finished {
        generation: u64,
        result: anyhow::Result<ExecutedCompaction>,
    },
}

impl TuiController {
    pub(super) fn new(
        runtime_host: AgentSessionRuntimeHost,
        options: SessionUiOptions,
        session_setup: SessionRuntimeSetup,
        command_tx: mpsc::UnboundedSender<bb_tui::tui::TuiCommand>,
        approval_rx: mpsc::UnboundedReceiver<PendingApprovalRequest>,
    ) -> Self {
        let resource_watch = ResourceWatchState::capture(&session_setup.tool_ctx.cwd);
        let (manual_compaction_tx, manual_compaction_rx) = mpsc::unbounded_channel();
        Self {
            runtime_host,
            session_setup,
            options,
            command_tx,
            abort_token: CancellationToken::new(),
            streaming: false,
            retry_status: None,
            queued_prompts: VecDeque::new(),
            pending_tree_summary_target: None,
            pending_tree_custom_prompt_target: None,
            pending_model_provider_search: None,
            pending_model_auth_selection: None,
            pending_login_api_key_provider: None,
            pending_login_copilot_enterprise: false,
            pending_images: Vec::new(),
            local_action_cancel: None,
            manual_compaction_in_progress: false,
            auto_compaction_in_progress: false,
            manual_compaction_generation: 0,
            manual_compaction_tx,
            manual_compaction_rx,
            color_theme: bb_tui::tui::spinner::ColorTheme::default(),
            shutdown_requested: false,
            approval_rx,
            pending_approval: None,
            session_approval_rules: HashSet::new(),
            pending_extension_menus: HashMap::new(),
            pending_extension_prompt: None,
            resource_watch,
            suppress_next_resource_watch_reload: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionApprovalRule, derive_session_approval_rule};

    #[test]
    fn derives_compound_prefix_for_git_like_commands() {
        assert_eq!(
            derive_session_approval_rule("git checkout -b feature"),
            SessionApprovalRule::Prefix("git checkout".to_string())
        );
        assert!(
            SessionApprovalRule::Prefix("git checkout".to_string()).matches("git checkout main")
        );
    }

    #[test]
    fn keeps_redirection_commands_exact_for_session_approvals() {
        assert_eq!(
            derive_session_approval_rule("echo hi > out.txt"),
            SessionApprovalRule::Exact("echo hi > out.txt".to_string())
        );
        assert!(
            !SessionApprovalRule::Exact("echo hi > out.txt".to_string())
                .matches("echo bye > out.txt")
        );
    }
}
