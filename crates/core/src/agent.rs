use crate::types::*;
use serde_json::Value;

/// Configuration for the agent loop.
pub struct AgentConfig {
    pub system_prompt: String,
    pub model_id: String,
    pub provider_name: String,
}

/// An event emitted by the agent loop.
#[derive(Clone, Debug)]
pub enum AgentEvent {
    TurnStart { turn_index: u32 },
    AssistantText { text: String },
    AssistantThinking { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallArgs { id: String, args: Value },
    ToolResult { id: String, result: String, is_error: bool },
    TurnEnd { turn_index: u32 },
    Done,
    Error { message: String },
}

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
            AssistantContent::ToolCall { id, name, arguments } => Some(PendingToolCall {
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
