use std::collections::HashMap;

use bb_core::types::ContentBlock;
use bb_session::{store::EntryRow, tree::TreeNode};

use super::transcript::BlockId;

use crate::select_list::SelectItem;

use super::transcript::Transcript;

#[derive(Clone, Debug, Default)]
pub struct FullscreenFooterData {
    pub line1: String,
    pub line2_left: String,
    pub line2_right: String,
}

#[derive(Clone, Debug)]
pub struct FullscreenAppConfig {
    pub title: String,
    pub input_placeholder: String,
    pub status_line: String,
    pub footer: FullscreenFooterData,
    pub transcript: Transcript,
    /// Extra slash-menu items from skills, prompts, and extension commands.
    pub extra_slash_items: Vec<SelectItem>,
    /// Working directory for `@` file completions.
    pub cwd: std::path::PathBuf,
}

impl Default for FullscreenAppConfig {
    fn default() -> Self {
        Self {
            title: "BB-Agent fullscreen transcript".to_string(),
            input_placeholder: "Type a prompt…".to_string(),
            status_line:
                "Ctrl+Shift+O tool expand • Enter submits • Shift+Enter inserts a newline • wheel scrolls transcript"
                    .to_string(),
            footer: FullscreenFooterData::default(),
            transcript: Transcript::new(),
            extra_slash_items: Vec::new(),
            cwd: std::env::current_dir().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FullscreenOutcome {
    pub submitted_inputs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FullscreenSubmission {
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
    CancelLocalAction,
}

#[derive(Clone, Debug)]
pub enum FullscreenNoteLevel {
    Status,
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
pub enum FullscreenCommand {
    SetStatusLine(String),
    SetFooter(FullscreenFooterData),
    SetTranscript(Transcript),
    SetTranscriptWithToolStates {
        transcript: Transcript,
        tool_states: HashMap<String, HistoricalToolState>,
    },
    SetInput(String),
    SetLocalActionActive(bool),
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
        level: FullscreenNoteLevel,
        text: String,
    },
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
pub enum FullscreenMode {
    #[default]
    Normal,
    Transcript,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FullscreenSearchState {
    pub query: String,
}
