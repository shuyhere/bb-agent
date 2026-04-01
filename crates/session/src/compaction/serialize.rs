use bb_core::types::{AgentMessage, AssistantContent, ContentBlock};

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
                        AssistantContent::ToolCall { name, arguments, .. } => {
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
                            if text.len() > 2000 {
                                out.push_str(&text[..2000]);
                                out.push_str("...(truncated)");
                            } else {
                                out.push_str(text);
                            }
                        }
                        ContentBlock::Image { .. } => out.push_str("[image]"),
                    }
                }
                out.push('\n');
            }
            AgentMessage::BashExecution(b) => {
                out.push_str(&format!("[Bash]: {}\n", b.command));
                let output = if b.output.len() > 2000 {
                    format!("{}...(truncated)", &b.output[..2000])
                } else {
                    b.output.clone()
                };
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
                            let truncated = if s.len() > 100 { &s[..100] } else { s };
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

