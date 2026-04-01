use std::path::PathBuf;

use anyhow::Result;
use bb_provider::registry::Model;
use bb_session::{store::SessionRow, tree::TreeNode};
use bb_tui::{
    model_selector::ModelSelector, session_selector::SessionSelector, tree_selector::TreeSelector,
};

/// Dedicated controller module for interactive slash/bang commands and selector flows.
///
/// This is a chunk-port scaffold from pi's interactive-mode command handling. The
/// shape intentionally mirrors pi's selector-opening helpers and command handler
/// grouping, while leaving integration points TODO-safe until the legacy interactive
/// loop is fully migrated into this module.
#[derive(Debug, Default)]
pub struct InteractiveCommands {
    pub(super) state: CommandUiState,
}

#[derive(Debug, Default, Clone)]
pub struct CommandUiState {
    pub active_selector: Option<SelectorKind>,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorKind {
    Settings,
    Model,
    Models,
    UserMessage,
    Tree,
    Session,
    OAuthLogin,
    OAuthLogout,
}

#[derive(Debug, Clone)]
pub enum SelectorRequest {
    Model { initial_search: Option<String> },
    Models,
    Tree { initial_selected_id: Option<String> },
    Session,
    OAuth { mode: OAuthMode },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthMode {
    Login,
    Logout,
}

pub enum SelectorOverlay {
    Model(ModelSelector),
    Session(SessionSelector),
    Tree(TreeSelector),
    Placeholder {
        kind: SelectorKind,
        title: &'static str,
    },
}

#[derive(Debug, Clone)]
pub enum SelectorAction {
    SetModel { provider: String, model_id: String },
    ResumeSession { session_id: String },
    NavigateTree { entry_id: String },
    Cancel,
    None,
}

#[derive(Debug, Clone)]
pub enum CommandAction {
    OpenSelector(SelectorRequest),
    Reload,
    Export {
        output_path: Option<PathBuf>,
        format: ExportFormat,
    },
    Import {
        input_path: PathBuf,
        replace_current: bool,
    },
    Share,
    CopyLastAssistantMessage,
    SetSessionName {
        name: Option<String>,
    },
    ShowSessionInfo,
    ShowChangelog,
    ShowHotkeys,
    ClearSession,
    Bash {
        command: String,
        exclude_from_context: bool,
    },
    Compact {
        custom_instructions: Option<String>,
    },
    Status(String),
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Html,
    Jsonl,
}

#[derive(Debug, Clone)]
pub struct SessionStatsView {
    pub session_file: Option<PathBuf>,
    pub session_id: String,
    pub session_name: Option<String>,
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub tool_calls: usize,
    pub tool_results: usize,
    pub total_messages: usize,
    pub tokens: TokenUsageView,
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsageView {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub total: u64,
}

#[derive(Debug, Clone)]
pub struct HotkeysView {
    pub navigation: Vec<HotkeyLine>,
    pub editing: Vec<HotkeyLine>,
    pub other: Vec<HotkeyLine>,
    pub extensions: Vec<HotkeyLine>,
}

#[derive(Debug, Clone)]
pub struct HotkeyLine {
    pub key: String,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct ReloadPlan {
    pub reload_keybindings: bool,
    pub reload_extensions: bool,
    pub reload_skills: bool,
    pub reload_prompts: bool,
    pub reload_themes: bool,
}

impl Default for ReloadPlan {
    fn default() -> Self {
        Self {
            reload_keybindings: true,
            reload_extensions: true,
            reload_skills: true,
            reload_prompts: true,
            reload_themes: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub output_path: Option<PathBuf>,
    pub format: ExportFormat,
}

#[derive(Debug, Clone)]
pub struct ImportRequest {
    pub input_path: PathBuf,
    pub replace_current: bool,
}

#[derive(Debug, Clone)]
pub struct ShareRequest {
    pub temp_export_path: PathBuf,
    pub gist_public: bool,
}

#[derive(Debug, Clone)]
pub struct BashRequest {
    pub command: String,
    pub exclude_from_context: bool,
    pub cwd: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CompactRequest {
    pub custom_instructions: Option<String>,
}

pub trait InteractiveCommandHost {
    fn current_model(&self) -> Option<Model>;
    fn available_models(&self) -> Vec<Model>;
    fn session_rows(&self) -> Vec<SessionRow>;
    fn session_tree(&self) -> Vec<TreeNode>;
    fn active_leaf_id(&self) -> Option<String>;
    fn current_working_directory(&self) -> PathBuf;
    fn max_selector_rows(&self) -> usize;

    fn set_status(&mut self, message: impl Into<String>);
    fn set_warning(&mut self, message: impl Into<String>);
    fn set_error(&mut self, message: impl Into<String>);

    fn reload_resources(&mut self, plan: ReloadPlan) -> Result<()>;
    fn export_session(&mut self, request: ExportRequest) -> Result<PathBuf>;
    fn import_session(&mut self, request: ImportRequest) -> Result<()>;
    fn share_session(&mut self, request: ShareRequest) -> Result<String>;
    fn copy_last_assistant_message(&mut self) -> Result<()>;
    fn set_session_name(&mut self, name: String) -> Result<()>;
    fn clear_session(&mut self) -> Result<()>;
    fn run_bash(&mut self, request: BashRequest) -> Result<()>;
    fn compact_session(&mut self, request: CompactRequest) -> Result<()>;
    fn session_stats(&self) -> Result<SessionStatsView>;
    fn changelog_markdown(&self) -> Result<String>;
    fn hotkeys_view(&self) -> HotkeysView;
}

