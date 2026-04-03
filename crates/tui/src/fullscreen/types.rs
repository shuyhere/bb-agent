use bb_core::types::ContentBlock;

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
}

impl Default for FullscreenAppConfig {
    fn default() -> Self {
        Self {
            title: "BB-Agent fullscreen transcript".to_string(),
            input_placeholder: "Type a prompt…".to_string(),
            status_line:
                "Ctrl+O transcript • Enter submits • Shift+Enter inserts a newline • wheel scrolls transcript"
                    .to_string(),
            footer: FullscreenFooterData::default(),
            transcript: Transcript::new(),
            extra_slash_items: Vec::new(),
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
    MenuSelection { menu_id: String, value: String },
}

#[derive(Clone, Debug)]
pub enum FullscreenNoteLevel {
    Status,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub enum FullscreenCommand {
    SetStatusLine(String),
    SetFooter(FullscreenFooterData),
    SetTranscript(Transcript),
    SetInput(String),
    OpenSelectMenu {
        menu_id: String,
        title: String,
        items: Vec<SelectItem>,
    },
    CloseSelectMenu,
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FullscreenMode {
    #[default]
    Normal,
    Transcript,
    Search,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FullscreenSearchState {
    pub query: String,
}
