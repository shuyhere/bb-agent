use crate::agent;
use serde_json::json;

pub fn messages_to_provider(messages: &[crate::types::AgentMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .filter_map(|msg| match msg {
            crate::types::AgentMessage::User(user) => {
                let has_images = user
                    .content
                    .iter()
                    .any(|block| matches!(block, crate::types::ContentBlock::Image { .. }));

                if has_images {
                    Some(json!({
                        "role": "user",
                        "content": content_blocks_to_provider(&user.content),
                    }))
                } else {
                    // Text-only: send as plain string (more compatible)
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
                let has_images = tool_result
                    .content
                    .iter()
                    .any(|block| matches!(block, crate::types::ContentBlock::Image { .. }));
                let content = if has_images {
                    json!(content_blocks_to_provider(&tool_result.content))
                } else {
                    json!(
                        tool_result
                            .content
                            .iter()
                            .filter_map(|block| match block {
                                crate::types::ContentBlock::Text { text } => Some(text.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                };
                Some(json!({
                    "role": "tool",
                    "tool_call_id": tool_result.tool_call_id,
                    "content": content,
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

fn content_blocks_to_provider(content: &[crate::types::ContentBlock]) -> Vec<serde_json::Value> {
    content
        .iter()
        .map(|block| match block {
            crate::types::ContentBlock::Text { text } => json!({
                "type": "text",
                "text": text
            }),
            crate::types::ContentBlock::Image { data, mime_type } => {
                json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": mime_type,
                        "data": data
                    }
                })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::messages_to_provider;
    use crate::types::{AgentMessage, ContentBlock, ToolResultMessage};

    #[test]
    fn tool_result_images_are_preserved_for_provider_conversion() {
        let messages = vec![AgentMessage::ToolResult(ToolResultMessage {
            tool_call_id: "call_1".to_string(),
            tool_name: "read".to_string(),
            content: vec![ContentBlock::Image {
                data: "iVBORw0KGgo=".to_string(),
                mime_type: "image/png".to_string(),
            }],
            details: None,
            is_error: false,
            timestamp: 0,
        })];

        let provider_messages = messages_to_provider(&messages);
        assert_eq!(provider_messages.len(), 1);
        assert_eq!(provider_messages[0]["role"], "tool");
        let content = provider_messages[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["source"]["media_type"], "image/png");
    }
}
