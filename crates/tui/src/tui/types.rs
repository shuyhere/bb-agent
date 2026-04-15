use std::collections::HashMap;

use bb_core::types::ContentBlock;
use bb_session::{store::EntryRow, tree::TreeNode};

use super::transcript::BlockId;

use crate::select_list::SelectItem;
use crate::ui_hints::TUI_TOOL_STATUS_HINT;

use super::transcript::Transcript;

#[derive(Clone, Debug, Default)]
pub struct TuiFooterData {
    pub line1: String,
    pub line2_left: String,
    pub line2_right: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TuiAuthStepState {
    Pending,
    Active,
    Done,
}

#[derive(Clone, Debug, Default)]
pub struct TuiAuthStep {
    pub label: String,
    pub state: Option<TuiAuthStepState>,
}

#[derive(Clone, Debug, Default)]
pub struct TuiAuthDialog {
    pub title: String,
    pub status: Option<String>,
    pub steps: Vec<TuiAuthStep>,
    pub url: Option<String>,
    pub lines: Vec<String>,
    pub input_label: Option<String>,
    pub input_placeholder: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TuiApprovalChoice {
    ApproveOnce,
    ApproveForSession,
    #[default]
    Deny,
}

#[derive(Clone, Debug, Default)]
pub struct TuiApprovalDialog {
    pub title: String,
    pub command: String,
    pub reason: String,
    pub lines: Vec<String>,
    pub allow_session: bool,
    pub session_scope_label: Option<String>,
    pub deny_input: String,
    pub deny_cursor: usize,
    pub deny_input_placeholder: Option<String>,
    pub selected: TuiApprovalChoice,
}

#[derive(Clone, Debug)]
pub struct TuiAppConfig {
    pub title: String,
    pub input_placeholder: String,
    pub status_line: String,
    pub footer: TuiFooterData,
    pub transcript: Transcript,
    /// Extra slash-menu items from skills, prompts, and extension commands.
    pub extra_slash_items: Vec<SelectItem>,
    /// Working directory for `@` file completions.
    pub cwd: std::path::PathBuf,
}

impl Default for TuiAppConfig {
    fn default() -> Self {
        Self {
            title: "BB-Agent TUI transcript".to_string(),
            input_placeholder: "Type a prompt…".to_string(),
            status_line: TUI_TOOL_STATUS_HINT.to_string(),
            footer: TuiFooterData::default(),
            transcript: Transcript::new(),
            extra_slash_items: Vec::new(),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TuiOutcome {
    pub submitted_inputs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TuiSubmission {
    Input(String),
    /// A prompt with attached image file paths.
    InputWithImages {
        text: String,
        image_paths: Vec<String>,
    },
    MenuSelection {
        menu_id: String,
        value: String,
    },
    ApprovalDecision {
        choice: TuiApprovalChoice,
        steer_message: Option<String>,
    },
    CancelLocalAction,
    EditQueuedMessages,
}

#[derive(Clone, Debug)]
pub enum TuiNoteLevel {
    Status,
    Highlight,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct HistoricalToolState {
    pub name: String,
    pub raw_args: String,
    pub tool_use_id: BlockId,
    pub tool_result_id: Option<BlockId>,
    pub result_content: Option<Vec<ContentBlock>>,
    pub result_details: Option<serde_json::Value>,
    pub artifact_path: Option<String>,
    pub is_error: bool,
}

#[derive(Clone, Debug)]
pub enum TuiCommand {
    SetStatusLine(String),
    SetFooter(TuiFooterData),
    SetTranscript(Transcript),
    SetTranscriptWithToolStates {
        transcript: Transcript,
        tool_states: HashMap<String, HistoricalToolState>,
    },
    SetInput(String),
    SetLocalActionActive(bool),
    OpenAuthDialog(TuiAuthDialog),
    UpdateAuthDialog(TuiAuthDialog),
    CloseAuthDialog,
    OpenApprovalDialog(TuiApprovalDialog),
    CloseApprovalDialog,
    SetExtraSlashItems(Vec<SelectItem>),
    OpenSelectMenu {
        menu_id: String,
        title: String,
        items: Vec<SelectItem>,
        selected_value: Option<String>,
    },
    OpenTreeMenu {
        menu_id: String,
        title: String,
        tree: Vec<TreeNode>,
        entries: Vec<EntryRow>,
        active_leaf: Option<String>,
        selected_value: Option<String>,
    },
    CloseSelectMenu,
    CloseTreeMenu,
    PushNote {
        level: TuiNoteLevel,
        text: String,
    },
    SetInputMonitor(Option<String>),
    TurnStart {
        turn_index: u32,
    },
    TextDelta(String),
    ThinkingDelta(String),
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        args: String,
    },
    ToolExecuting {
        id: String,
    },
    ToolOutputDelta {
        id: String,
        chunk: String,
    },
    ToolResult {
        id: String,
        name: String,
        content: Vec<ContentBlock>,
        details: Option<serde_json::Value>,
        artifact_path: Option<String>,
        is_error: bool,
    },
    TurnEnd,
    TurnAborted,
    TurnError {
        message: String,
    },
    SetColorTheme(super::spinner::ColorTheme),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TuiMode {
    #[default]
    Normal,
    Transcript,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TuiSearchState {
    pub query: String,
}
