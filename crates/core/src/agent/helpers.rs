use std::sync::Arc;

use serde_json::Value;

use crate::types::AssistantContent;

use super::callbacks::ConvertToLlmFn;
use super::data::{AgentContextSnapshot, AgentMessage, AgentMessageRole};

/// A pending tool call from the assistant.
#[derive(Clone, Debug)]
pub struct PendingToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Parse tool calls from assistant content.
pub fn extract_tool_calls(content: &[AssistantContent]) -> Vec<PendingToolCall> {
    content
        .iter()
        .filter_map(|c| match c {
            AssistantContent::ToolCall {
                id,
                name,
                arguments,
            } => Some(PendingToolCall {
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// Extract text from assistant content.
pub fn extract_text(content: &[AssistantContent]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            AssistantContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Build the system prompt from base prompt + AGENTS.md content.
pub fn build_system_prompt(base: &str, agents_md: Option<&str>) -> String {
    match agents_md {
        Some(md) if !md.is_empty() => format!("{base}\n\n{md}"),
        _ => base.to_string(),
    }
}

/// The default minimal system prompt.
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are an expert coding assistant. You help users by reading files, executing commands, editing code, and writing new files.

Available tools:
- read: Read file contents (text and images), with offset/limit for large files
- bash: Execute bash commands with optional timeout
- edit: Make precise edits with exact text replacement
- write: Create or overwrite files

Guidelines:
- Use bash for file operations like ls, grep, find
- Use read to examine files before editing
- Use edit for precise changes (old text must match exactly)
- Use write only for new files or complete rewrites
- Be concise in your responses
- Show file paths clearly when working with files"#;

pub(crate) fn context_with_prompt(
    mut context: AgentContextSnapshot,
    messages: Vec<AgentMessage>,
) -> AgentContextSnapshot {
    context.messages.extend(messages);
    context
}

pub(crate) fn default_convert_to_llm() -> ConvertToLlmFn {
    Arc::new(|messages| {
        Box::pin(async move {
            messages
                .into_iter()
                .filter(|message| {
                    matches!(
                        message.role,
                        AgentMessageRole::User
                            | AgentMessageRole::Assistant
                            | AgentMessageRole::ToolResult
                    )
                })
                .collect()
        })
    })
}

pub(crate) fn default_stream_fn() -> super::callbacks::StreamFn {
    Arc::new(|_context, _config, _sink, _signal| {
        Box::pin(async move {
            anyhow::bail!(
                "Agent::stream_fn is not implemented in bb-core yet; provide a runtime loop placeholder"
            )
        })
    })
}
