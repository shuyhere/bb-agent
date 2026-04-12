use bb_core::types::{AgentMessage, AssistantContent, ContentBlock};

fn truncate_utf8(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let prefix: String = text.chars().take(max_chars).collect();
    format!("{prefix}...(truncated)")
}

// =============================================================================
// Conversation serialization
// =============================================================================

/// Serialize messages to text for the summarizer LLM.
/// Format:
///   [User]: message text
///   [Assistant]: response text
///   [Assistant tool calls]: read(path="..."); bash(command="...")
///   [Tool result]: output text (truncated to 2000 chars)
pub fn serialize_conversation(messages: &[AgentMessage]) -> String {
    let mut out = String::new();
    for msg in messages {
        match msg {
            AgentMessage::User(u) => {
                out.push_str("[User]: ");
                for block in &u.content {
                    match block {
                        ContentBlock::Text { text } => out.push_str(text),
                        ContentBlock::Image { .. } => out.push_str("[image]"),
                    }
                }
                out.push('\n');
            }
            AgentMessage::Assistant(a) => {
                let mut text_parts = Vec::new();
                let mut tool_parts = Vec::new();
                for block in &a.content {
                    match block {
                        AssistantContent::Text { text } => text_parts.push(text.clone()),
                        AssistantContent::Thinking { .. } => {}
                        AssistantContent::ToolCall {
                            name, arguments, ..
                        } => {
                            let args_str = format_tool_args(name, arguments);
                            tool_parts.push(format!("{name}({args_str})"));
                        }
                    }
                }
                if !text_parts.is_empty() {
                    out.push_str("[Assistant]: ");
                    out.push_str(&text_parts.join("\n"));
                    out.push('\n');
                }
                if !tool_parts.is_empty() {
                    out.push_str("[Assistant tool calls]: ");
                    out.push_str(&tool_parts.join("; "));
                    out.push('\n');
                }
            }
            AgentMessage::ToolResult(tr) => {
                out.push_str("[Tool result]: ");
                for block in &tr.content {
                    match block {
                        ContentBlock::Text { text } => {
                            out.push_str(&truncate_utf8(text, 2000));
                        }
                        ContentBlock::Image { .. } => out.push_str("[image]"),
                    }
                }
                out.push('\n');
            }
            AgentMessage::BashExecution(b) => {
                out.push_str(&format!("[Bash]: {}\n", b.command));
                let output = truncate_utf8(&b.output, 2000);
                out.push_str(&format!("[Bash output]: {output}\n"));
            }
            AgentMessage::Custom(c) => {
                out.push_str(&format!("[Custom/{}]: ", c.custom_type));
                for block in &c.content {
                    match block {
                        ContentBlock::Text { text } => out.push_str(text),
                        ContentBlock::Image { .. } => out.push_str("[image]"),
                    }
                }
                out.push('\n');
            }
            AgentMessage::BranchSummary(bs) => {
                out.push_str(&format!("[Branch summary]: {}\n", bs.summary));
            }
            AgentMessage::CompactionSummary(cs) => {
                out.push_str(&format!("[Previous summary]: {}\n", cs.summary));
            }
        }
    }
    out
}

fn format_tool_args(_name: &str, arguments: &serde_json::Value) -> String {
    match arguments.as_object() {
        Some(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    let val = match v.as_str() {
                        Some(s) => {
                            let truncated = if s.chars().count() > 100 {
                                s.chars().take(100).collect::<String>()
                            } else {
                                s.to_string()
                            };
                            format!("\"{truncated}\"")
                        }
                        None => v.to_string(),
                    };
                    format!("{k}={val}")
                })
                .collect();
            pairs.join(", ")
        }
        None => arguments.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::types::{BashExecutionMessage, ToolResultMessage};
    use serde_json::json;

    #[test]
    fn serialize_conversation_truncates_tool_results_on_char_boundaries() {
        let text = format!("{}—suffix", "a".repeat(1998));
        let messages = vec![AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "tool-1".to_string(),
            tool_name: "read".to_string(),
            content: vec![ContentBlock::Text { text }],
            details: None,
            is_error: false,
            timestamp: 0,
        })];

        let serialized = serialize_conversation(&messages);
        assert!(serialized.contains("...(truncated)"));
        assert!(serialized.contains('—'));
    }

    #[test]
    fn serialize_conversation_truncates_bash_output_on_char_boundaries() {
        let output = format!("{}—suffix", "b".repeat(1998));
        let messages = vec![AgentMessage::BashExecution(BashExecutionMessage {
            command: "echo hi".to_string(),
            output,
            exit_code: Some(0),
            cancelled: false,
            truncated: false,
            full_output_path: None,
            timestamp: 0,
        })];

        let serialized = serialize_conversation(&messages);
        assert!(serialized.contains("...(truncated)"));
        assert!(serialized.contains('—'));
    }

    #[test]
    fn format_tool_args_truncates_strings_on_char_boundaries() {
        let args = json!({
            "text": format!("{}—suffix", "c".repeat(98))
        });

        let formatted = format_tool_args("write", &args);
        assert!(formatted.contains('—'));
        assert!(!formatted.contains("suffix"));
    }
}
