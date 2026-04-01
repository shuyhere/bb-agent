use crate::interactive::controller::components::assistant_message::{
    AssistantMessage, AssistantMessageContent, AssistantStopReason,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreQueuedMessagesResult {
    pub restored_count: usize,
    pub editor_text: String,
}

pub fn assistant_message_from_parts(
    text: impl Into<String>,
    thinking: Option<String>,
    has_tool_call: bool,
) -> AssistantMessage {
    let mut content = Vec::new();
    if let Some(thinking) = thinking {
        if !thinking.trim().is_empty() {
            content.push(AssistantMessageContent::Thinking(thinking));
        }
    }

    let text = text.into();
    if !text.trim().is_empty() {
        content.push(AssistantMessageContent::Text(text));
    }

    if has_tool_call {
        content.push(AssistantMessageContent::ToolCall);
    }

    AssistantMessage {
        content,
        stop_reason: Some(AssistantStopReason::Other),
        error_message: None,
    }
}
