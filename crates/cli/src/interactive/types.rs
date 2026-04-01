use super::events::{InteractiveRenderState, PendingMessages};
use super::interactive_commands::InteractiveCommands;
use bb_core::agent_session_runtime::AgentSessionRuntimeHost;
use bb_provider::Provider;
use bb_tools::{Tool, ToolContext};
use crossterm::event::{KeyCode, KeyModifiers};
use std::error::Error;

pub type InteractiveResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, Default)]
pub struct InteractiveModeOptions {
    pub verbose: bool,
    pub quiet_startup: bool,
    pub migrated_providers: Vec<String>,
    pub model_fallback_message: Option<String>,
    pub initial_message: Option<String>,
    pub initial_images: Vec<String>,
    pub initial_messages: Vec<String>,
    pub session_id: Option<String>,
    pub model_display: Option<String>,
    pub agents_md: Option<String>,
}

/// Non-Clone runtime state needed for actual LLM calls.
pub struct InteractiveSessionSetup {
    pub conn: rusqlite::Connection,
    pub session_id: String,
    pub provider: Box<dyn Provider>,
    pub model: bb_provider::registry::Model,
    pub api_key: String,
    pub base_url: String,
    pub tools: Vec<Box<dyn Tool>>,
    pub tool_defs: Vec<serde_json::Value>,
    pub tool_ctx: ToolContext,
    pub system_prompt: String,
    pub thinking_level: String,
}

#[derive(Debug, Default)]
pub(super) struct InteractiveSessionState {
    pub(super) render_state: InteractiveRenderState,
    pub(super) pending_messages: PendingMessages,
}

pub(super) struct InteractiveController {
    pub(super) runtime_host: AgentSessionRuntimeHost,
    pub(super) session: InteractiveSessionState,
    pub(super) commands: InteractiveCommands,
}

impl InteractiveController {
    pub(super) fn new(runtime_host: AgentSessionRuntimeHost) -> Self {
        Self {
            runtime_host,
            session: InteractiveSessionState::default(),
            commands: InteractiveCommands::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct KeyBinding {
    pub(super) code: KeyCode,
    pub(super) modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KeyAction {
    Escape,
    ClearOrInterrupt,
    ExitEmpty,
    Suspend,
    CycleThinking,
    CycleModelForward,
    CycleModelBackward,
    SelectModel,
    ToggleToolExpansion,
    ToggleThinkingVisibility,
    OpenExternalEditor,
    FollowUp,
    Dequeue,
    SessionNew,
    SessionTree,
    SessionFork,
    SessionResume,
    PasteImage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SubmitAction {
    Settings,
    ScopedModels,
    Model,
    Export,
    Import,
    Share,
    Copy,
    Name,
    Session,
    Changelog,
    Hotkeys,
    Fork,
    Tree,
    Login,
    Logout,
    New,
    Compact,
    Reload,
    Debug,
    ArminSaysHi,
    Resume,
    Quit,
    Help,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SubmitMatch {
    Exact(&'static str),
    Prefix(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct SubmitRoute {
    pub(super) matcher: SubmitMatch,
    pub(super) action: SubmitAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SubmitOutcome {
    Ignored,
    Submitted,
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum QueuedMessageKind {
    Steer,
    FollowUp,
}

impl Default for QueuedMessageKind {
    fn default() -> Self {
        Self::Steer
    }
}

#[derive(Debug, Default)]
pub(super) struct QueuedMessage {
    pub(super) text: String,
    pub(super) kind: QueuedMessageKind,
}
