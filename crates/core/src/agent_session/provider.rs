use crate::agent;

pub fn messages_to_provider(messages: &[crate::types::AgentMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter_map(|msg| match msg {
            crate::types::AgentMessage::User(user) => {
                let text = user
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::types::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(serde_json::json!({"role": "user", "content": text}))
            }
            crate::types::AgentMessage::Assistant(assistant) => {
                let text = agent::extract_text(&assistant.content);
                let tool_calls: Vec<serde_json::Value> = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::types::AssistantContent::ToolCall {
                            id,
                            name,
                            arguments,
                        } => Some(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(arguments).unwrap_or_default()
                            }
                        })),
                        _ => None,
                    })
                    .collect();
                let mut msg = serde_json::json!({"role": "assistant"});
                if !text.is_empty() {
                    msg["content"] = serde_json::json!(text);
                }
                if !tool_calls.is_empty() {
                    msg["tool_calls"] = serde_json::json!(tool_calls);
                }
                Some(msg)
            }
            crate::types::AgentMessage::ToolResult(tool_result) => {
                let text = tool_result
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::types::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Some(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_result.tool_call_id,
                    "content": text,
                }))
            }
            crate::types::AgentMessage::Custom(custom) => {
                let text = custom
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::types::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (!text.is_empty()).then(|| serde_json::json!({"role": "user", "content": text}))
            }
            crate::types::AgentMessage::CompactionSummary(compaction) => Some(serde_json::json!({
                "role": "user",
                "content": format!("[Previous conversation summary]\n\n{}", compaction.summary),
            })),
            crate::types::AgentMessage::BranchSummary(branch) => Some(serde_json::json!({
                "role": "user",
                "content": format!("[Branch summary]\n\n{}", branch.summary),
            })),
            _ => None,
        })
        .collect()
}
