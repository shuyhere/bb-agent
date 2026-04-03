use bb_core::types::{AgentMessage, AssistantContent, ContentBlock, SessionEntry};
use serde_json::Value;

pub(super) fn text_from_blocks(blocks: &[ContentBlock], separator: &str) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(separator)
}

pub(super) fn format_assistant_text(message: &bb_core::types::AssistantMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            AssistantContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn format_tool_arguments(arguments: &serde_json::Value) -> String {
    match arguments {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    }
}

pub(super) fn format_tool_result_blocks(blocks: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        match block {
            ContentBlock::Text { text } => parts.push(text.clone()),
            ContentBlock::Image { mime_type, .. } => {
                parts.push(format!("[image: {mime_type}]"));
            }
        }
    }
    parts.join("\n")
}

pub(super) fn tree_entry_role_and_preview(entry: &SessionEntry) -> (String, String) {
    match entry {
        SessionEntry::Message {
            message: AgentMessage::User(user),
            ..
        } => ("user".to_string(), text_from_blocks(&user.content, " ")),
        SessionEntry::Message {
            message: AgentMessage::Assistant(msg),
            ..
        } => {
            let text = format_assistant_text(msg);
            (
                "assistant".to_string(),
                if text.is_empty() {
                    "assistant".to_string()
                } else {
                    text
                },
            )
        }
        SessionEntry::Message {
            message: AgentMessage::ToolResult(result),
            ..
        } => (
            "tool_result".to_string(),
            format_tool_result_blocks(&result.content),
        ),
        SessionEntry::Compaction { summary, .. } => ("compaction".to_string(), summary.clone()),
        SessionEntry::BranchSummary { summary, .. } => {
            ("branch_summary".to_string(), summary.clone())
        }
        SessionEntry::CustomMessage { custom_type, .. } => {
            ("custom".to_string(), custom_type.clone())
        }
        _ => ("other".to_string(), String::new()),
    }
}

#[allow(dead_code)]
pub(super) fn format_tool_result_content(
    content: &[ContentBlock],
    details: Option<&Value>,
    artifact_path: Option<&str>,
) -> String {
    let mut sections = Vec::new();

    let mut rendered_content = String::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => {
                if !rendered_content.is_empty() {
                    rendered_content.push('\n');
                }
                rendered_content.push_str(text);
            }
            ContentBlock::Image { mime_type, .. } => {
                if !rendered_content.is_empty() {
                    rendered_content.push('\n');
                }
                rendered_content.push_str(&format!("[image output: {mime_type}]"));
            }
        }
    }
    if !rendered_content.trim().is_empty() {
        sections.push(rendered_content);
    }

    if let Some(details) = details {
        let details =
            serde_json::to_string_pretty(details).unwrap_or_else(|_| details.to_string());
        sections.push(format!("details:\n{details}"));
    }

    if let Some(path) = artifact_path {
        sections.push(format!("artifact: {path}"));
    }

    if sections.is_empty() {
        "(no textual output)".to_string()
    } else {
        sections.join("\n\n")
    }
}
