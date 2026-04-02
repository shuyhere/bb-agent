use std::collections::BTreeMap;

use serde_json::Value;

use crate::interactive::controller::components::assistant_message::{
    AssistantMessage, AssistantMessageComponent,
};
use crate::interactive::controller::components::bash_execution::BashExecutionComponent;
use crate::interactive::controller::components::tool_execution::{
    ToolExecutionComponent, ToolExecutionResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedMessageMode {
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub text: String,
    pub mode: QueuedMessageMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingMessages {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}

impl PendingMessages {
    pub fn is_empty(&self) -> bool {
        self.steering.is_empty() && self.follow_up.is_empty()
    }

    pub fn combined(&self) -> Vec<String> {
        self.steering
            .iter()
            .chain(self.follow_up.iter())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone)]
pub enum ChatItem {
    Spacer,
    UserMessage(String),
    AssistantMessage(AssistantMessageComponent),
    ToolExecution(ToolExecutionComponent),
    BashExecution(BashExecutionComponent),
    CustomMessage {
        custom_type: String,
        display: bool,
        text: String,
    },
    CompactionSummary(String),
    BranchSummary(String),
    PendingMessageLine(String),
    SystemMessage(String),
}

#[derive(Debug, Clone)]
pub struct ToolCallContent {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub enum InteractiveMessage {
    User {
        text: String,
    },
    Assistant {
        message: AssistantMessage,
        tool_calls: Vec<ToolCallContent>,
    },
    ToolResult {
        tool_call_id: String,
        result: ToolExecutionResult,
    },
    BashExecution {
        command: String,
        output: Option<String>,
        exit_code: Option<i32>,
        cancelled: bool,
        truncated: bool,
        full_output_path: Option<String>,
        exclude_from_context: bool,
    },
    Custom {
        custom_type: String,
        text: String,
        display: bool,
    },
    CompactionSummary {
        summary: String,
    },
    BranchSummary {
        summary: String,
    },
    /// System/error message shown inline in chat.
    System {
        text: String,
    },
}

#[derive(Debug, Clone)]
pub struct SessionContext {
    pub messages: Vec<InteractiveMessage>,
}

#[derive(Debug, Clone)]
pub enum InteractiveSessionEvent {
    AgentStart,
    QueueUpdate,
    MessageStart {
        message: InteractiveMessage,
    },
    MessageUpdate {
        message: InteractiveMessage,
    },
    MessageEnd {
        message: InteractiveMessage,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: Value,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        partial_result: ToolExecutionResult,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        result: ToolExecutionResult,
        is_error: bool,
    },
    AgentEnd,
    CompactionStart,
    CompactionEnd {
        summary: Option<String>,
        error_message: Option<String>,
    },
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
    },
    AutoRetryEnd {
        success: bool,
        attempt: u32,
        final_error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct InteractiveRenderState {
    pub chat_items: Vec<ChatItem>,
    pub pending_items: Vec<ChatItem>,
    pub pending_tools: BTreeMap<String, ToolExecutionComponent>,
    pub streaming_component: Option<AssistantMessageComponent>,
    pub streaming_message: Option<AssistantMessage>,
    pub pending_working_message: Option<String>,
    pub retry_attempt: usize,
    pub tool_output_expanded: bool,
    pub hide_thinking_block: bool,
    pub hidden_thinking_label: String,
    pub show_images: bool,
    pub last_status: Option<String>,
}

impl Default for InteractiveRenderState {
    fn default() -> Self {
        Self {
            chat_items: Vec::new(),
            pending_items: Vec::new(),
            pending_tools: BTreeMap::new(),
            streaming_component: None,
            streaming_message: None,
            pending_working_message: None,
            retry_attempt: 0,
            tool_output_expanded: false,
            hide_thinking_block: false,
            hidden_thinking_label: "Thinking...".to_string(),
            show_images: true,
            last_status: None,
        }
    }
}

