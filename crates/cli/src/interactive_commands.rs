use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use bb_provider::registry::{Model, ModelRegistry};
use bb_session::{store::SessionRow, tree::TreeNode};
use bb_tui::{model_selector::ModelSelector, session_selector::SessionSelector, tree_selector::TreeSelector};

/// Dedicated controller module for interactive slash/bang commands and selector flows.
///
/// This is a chunk-port scaffold from pi's interactive-mode command handling. The
/// shape intentionally mirrors pi's selector-opening helpers and command handler
/// grouping, while leaving integration points TODO-safe until the legacy interactive
/// loop is fully migrated into this module.
#[derive(Debug, Default)]
pub struct InteractiveCommands {
    state: CommandUiState,
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
    Export { output_path: Option<PathBuf>, format: ExportFormat },
    Import { input_path: PathBuf, replace_current: bool },
    Share,
    CopyLastAssistantMessage,
    SetSessionName { name: Option<String> },
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

impl InteractiveCommands {
    pub fn new() -> Self {
        Self::default()
    }

    /// pi: showSelector(create)
    ///
    /// Rust port: mark the active selector and return the constructed overlay for the
    /// caller to mount into the TUI.
    pub fn show_selector(&mut self, overlay: SelectorOverlay) -> SelectorOverlay {
        self.state.active_selector = Some(match &overlay {
            SelectorOverlay::Model(_) => SelectorKind::Model,
            SelectorOverlay::Session(_) => SelectorKind::Session,
            SelectorOverlay::Tree(_) => SelectorKind::Tree,
            SelectorOverlay::Placeholder { kind, .. } => kind.clone(),
        });
        overlay
    }

    pub fn dismiss_selector(&mut self) {
        self.state.active_selector = None;
    }

    pub fn active_selector(&self) -> Option<&SelectorKind> {
        self.state.active_selector.as_ref()
    }

    pub fn handle_model_command<H: InteractiveCommandHost>(
        &mut self,
        host: &mut H,
        search_term: Option<&str>,
    ) -> Result<CommandAction> {
        if let Some(search_term) = search_term.map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(model) = self.find_exact_model_match(host.available_models(), search_term) {
                host.set_status(format!("Model: {}/{}", model.provider, model.id));
                return Ok(CommandAction::Status(format!(
                    "model-match:{}/{}",
                    model.provider, model.id
                )));
            }
            return Ok(CommandAction::OpenSelector(SelectorRequest::Model {
                initial_search: Some(search_term.to_string()),
            }));
        }

        Ok(CommandAction::OpenSelector(SelectorRequest::Model {
            initial_search: None,
        }))
    }

    pub fn find_exact_model_match(&self, models: Vec<Model>, search_term: &str) -> Option<Model> {
        let needle = search_term.trim().to_ascii_lowercase();
        models.into_iter().find(|model| {
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            model.id.eq_ignore_ascii_case(&needle)
                || model.name.eq_ignore_ascii_case(&needle)
                || provider_id == needle
                || provider_colon_id == needle
        })
    }

    pub fn open_model_selector(
        &mut self,
        registry: &ModelRegistry,
        initial_search_input: Option<&str>,
        max_visible: usize,
    ) -> SelectorOverlay {
        let mut selector = ModelSelector::new(registry, max_visible);
        if let Some(query) = initial_search_input.filter(|q| !q.is_empty()) {
            selector.set_search(query);
        }
        self.show_selector(SelectorOverlay::Model(selector))
    }

    pub fn open_session_selector(&mut self, sessions: Vec<SessionRow>, max_visible: usize) -> SelectorOverlay {
        self.show_selector(SelectorOverlay::Session(SessionSelector::new(sessions, max_visible)))
    }

    pub fn open_tree_selector(
        &mut self,
        tree: Vec<TreeNode>,
        active_leaf: Option<&str>,
        initial_selected_id: Option<&str>,
        max_visible: usize,
    ) -> SelectorOverlay {
        let selector = TreeSelector::new(tree, active_leaf, max_visible);
        let _ = initial_selected_id;
        // TODO(chunk/r26): seed initial selection by entry id once bb-tui TreeSelector
        // exposes an API equivalent to pi's TreeSelectorComponent initialSelectedId.
        self.show_selector(SelectorOverlay::Tree(selector))
    }

    pub fn open_placeholder_selector(&mut self, kind: SelectorKind, title: &'static str) -> SelectorOverlay {
        self.show_selector(SelectorOverlay::Placeholder { kind, title })
    }

    pub fn show_settings_selector(&mut self) -> SelectorOverlay {
        self.open_placeholder_selector(SelectorKind::Settings, "Settings")
    }

    pub fn show_models_selector(&mut self) -> SelectorOverlay {
        self.open_placeholder_selector(SelectorKind::Models, "Scoped Models")
    }

    pub fn show_user_message_selector(&mut self) -> SelectorOverlay {
        self.open_placeholder_selector(SelectorKind::UserMessage, "User Message Selector")
    }

    pub fn show_oauth_selector(&mut self, mode: OAuthMode) -> SelectorOverlay {
        let (kind, title) = match mode {
            OAuthMode::Login => (SelectorKind::OAuthLogin, "Login"),
            OAuthMode::Logout => (SelectorKind::OAuthLogout, "Logout"),
        };
        self.open_placeholder_selector(kind, title)
    }

    pub fn on_model_selected(&mut self, provider: String, model_id: String) -> SelectorAction {
        self.dismiss_selector();
        SelectorAction::SetModel { provider, model_id }
    }

    pub fn on_session_selected(&mut self, session_id: String) -> SelectorAction {
        self.dismiss_selector();
        SelectorAction::ResumeSession { session_id }
    }

    pub fn on_tree_selected(&mut self, entry_id: String) -> SelectorAction {
        self.dismiss_selector();
        SelectorAction::NavigateTree { entry_id }
    }

    pub fn on_selector_cancelled(&mut self) -> SelectorAction {
        self.dismiss_selector();
        SelectorAction::Cancel
    }

    pub fn handle_reload_command<H: InteractiveCommandHost>(&mut self, host: &mut H) -> Result<CommandAction> {
        host.reload_resources(ReloadPlan::default())?;
        host.set_status("Reloaded keybindings, extensions, skills, prompts, themes");
        Ok(CommandAction::Reload)
    }

    pub fn parse_export_command(&self, text: &str) -> ExportRequest {
        let output_path = text
            .split_whitespace()
            .nth(1)
            .map(PathBuf::from);
        let format = match output_path.as_ref().and_then(|path| path.extension()).and_then(|ext| ext.to_str()) {
            Some("jsonl") => ExportFormat::Jsonl,
            _ => ExportFormat::Html,
        };
        ExportRequest { output_path, format }
    }

    pub fn handle_export_command<H: InteractiveCommandHost>(
        &mut self,
        host: &mut H,
        text: &str,
    ) -> Result<CommandAction> {
        let request = self.parse_export_command(text);
        let file_path = host.export_session(request.clone())?;
        host.set_status(format!("Session exported to: {}", file_path.display()));
        Ok(CommandAction::Export {
            output_path: request.output_path,
            format: request.format,
        })
    }

    pub fn parse_import_command(&self, text: &str) -> Result<ImportRequest> {
        let input_path = text
            .split_whitespace()
            .nth(1)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("Usage: /import <path.jsonl>"))?;
        Ok(ImportRequest {
            input_path,
            replace_current: true,
        })
    }

    pub fn handle_import_command<H: InteractiveCommandHost>(
        &mut self,
        host: &mut H,
        text: &str,
    ) -> Result<CommandAction> {
        let request = self.parse_import_command(text)?;
        host.import_session(request.clone())?;
        host.set_status(format!("Session imported from: {}", request.input_path.display()));
        Ok(CommandAction::Import {
            input_path: request.input_path,
            replace_current: request.replace_current,
        })
    }

    pub fn default_share_request(&self) -> ShareRequest {
        ShareRequest {
            temp_export_path: std::env::temp_dir().join("session.html"),
            gist_public: false,
        }
    }

    pub fn handle_share_command<H: InteractiveCommandHost>(&mut self, host: &mut H) -> Result<CommandAction> {
        let url = host.share_session(self.default_share_request())?;
        host.set_status(format!("Share URL: {url}"));
        Ok(CommandAction::Share)
    }

    pub fn handle_copy_command<H: InteractiveCommandHost>(&mut self, host: &mut H) -> Result<CommandAction> {
        host.copy_last_assistant_message()?;
        host.set_status("Copied last agent message to clipboard");
        Ok(CommandAction::CopyLastAssistantMessage)
    }

    pub fn parse_name_command(&self, text: &str) -> Option<String> {
        text.strip_prefix("/name")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
    }

    pub fn handle_name_command<H: InteractiveCommandHost>(
        &mut self,
        host: &mut H,
        text: &str,
    ) -> Result<CommandAction> {
        let name = self.parse_name_command(text);
        if let Some(name_value) = name.as_ref() {
            host.set_session_name(name_value.clone())?;
            host.set_status(format!("Session name set: {name_value}"));
        } else {
            host.set_warning("Usage: /name <name>");
        }
        Ok(CommandAction::SetSessionName { name })
    }

    pub fn handle_session_command<H: InteractiveCommandHost>(&mut self, host: &mut H) -> Result<SessionStatsView> {
        host.session_stats()
    }

    pub fn handle_changelog_command<H: InteractiveCommandHost>(&mut self, host: &mut H) -> Result<String> {
        host.changelog_markdown()
    }

    pub fn capitalize_key(&self, key: &str) -> String {
        key.split('/')
            .map(|segment| {
                segment
                    .split('+')
                    .map(|part| {
                        let mut chars = part.chars();
                        match chars.next() {
                            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                            None => String::new(),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("+")
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    pub fn handle_hotkeys_command<H: InteractiveCommandHost>(&mut self, host: &mut H) -> HotkeysView {
        host.hotkeys_view()
    }

    pub fn handle_clear_command<H: InteractiveCommandHost>(&mut self, host: &mut H) -> Result<CommandAction> {
        host.clear_session()?;
        host.set_status("✓ New session started");
        Ok(CommandAction::ClearSession)
    }

    pub fn handle_bash_command<H: InteractiveCommandHost>(
        &mut self,
        host: &mut H,
        command: impl Into<String>,
        exclude_from_context: bool,
    ) -> Result<CommandAction> {
        let request = BashRequest {
            command: command.into(),
            exclude_from_context,
            cwd: host.current_working_directory(),
        };
        host.run_bash(request.clone())?;
        Ok(CommandAction::Bash {
            command: request.command,
            exclude_from_context: request.exclude_from_context,
        })
    }

    pub fn handle_compact_command<H: InteractiveCommandHost>(
        &mut self,
        host: &mut H,
        custom_instructions: Option<String>,
    ) -> Result<CommandAction> {
        let request = CompactRequest { custom_instructions };
        host.compact_session(request.clone())?;
        host.set_status("Compaction started");
        Ok(CommandAction::Compact {
            custom_instructions: request.custom_instructions,
        })
    }
}

pub fn export_format_from_path(path: Option<&Path>) -> ExportFormat {
    match path
        .and_then(|path| path.extension())
        .and_then(|ext| ext.to_str())
    {
        Some("jsonl") => ExportFormat::Jsonl,
        _ => ExportFormat::Html,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_format_defaults_to_html() {
        assert_eq!(export_format_from_path(None), ExportFormat::Html);
        assert_eq!(
            export_format_from_path(Some(Path::new("session.html"))),
            ExportFormat::Html
        );
    }

    #[test]
    fn export_format_detects_jsonl() {
        assert_eq!(
            export_format_from_path(Some(Path::new("session.jsonl"))),
            ExportFormat::Jsonl
        );
    }

    #[test]
    fn capitalize_key_title_cases_parts() {
        let controller = InteractiveCommands::new();
        assert_eq!(controller.capitalize_key("ctrl+c"), "Ctrl+C");
        assert_eq!(controller.capitalize_key("ctrl+k/ctrl+d"), "Ctrl+K/Ctrl+D");
    }

    #[test]
    fn parse_name_command_ignores_empty_name() {
        let controller = InteractiveCommands::new();
        assert_eq!(controller.parse_name_command("/name"), None);
        assert_eq!(controller.parse_name_command("/name   "), None);
        assert_eq!(
            controller.parse_name_command("/name chunk port"),
            Some("chunk port".into())
        );
    }
}
