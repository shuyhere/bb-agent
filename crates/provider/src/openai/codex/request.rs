use serde_json::{Value, json};

pub(super) fn resolve_codex_url(base_url: &str) -> String {
    let raw = if base_url.trim().is_empty() || base_url.contains("api.openai.com") {
        "https://chatgpt.com/backend-api".to_string()
    } else {
        base_url.trim_end_matches('/').to_string()
    };
    if raw.ends_with("/codex/responses") {
        raw
    } else if raw.ends_with("/codex") {
        format!("{raw}/responses")
    } else {
        format!("{raw}/codex/responses")
    }
}

pub(super) fn codex_reasoning_effort(thinking: &str) -> &'static str {
    match thinking {
        "low" | "minimal" => "low",
        "medium" => "medium",
        "high" | "xhigh" => "high",
        _ => "medium",
    }
}

pub(super) fn convert_tools_for_codex(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let func = tool.get("function")?;
            Some(json!({
                "type": "function",
                "name": func.get("name").and_then(|v| v.as_str()).unwrap_or("tool"),
                "description": func.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                "parameters": func.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"})),
            }))
        })
        .collect()
}

pub(super) fn convert_messages_for_codex(messages: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for (idx, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        match role {
            "user" => {
                if let Some(arr) = msg.get("content").and_then(|v| v.as_array()) {
                    let mut content = Vec::new();
                    for block in arr {
                        match block.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                            "text" => {
                                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                    content.push(json!({
                                        "type": "input_text",
                                        "text": text,
                                    }));
                                }
                            }
                            "image" => {
                                let media_type = block
                                    .get("source")
                                    .and_then(|s| s.get("media_type"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("image/png");
                                let data = block
                                    .get("source")
                                    .and_then(|s| s.get("data"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                content.push(json!({
                                    "type": "input_image",
                                    "image_url": format!("data:{media_type};base64,{data}"),
                                    "detail": "high",
                                }));
                            }
                            _ => {}
                        }
                    }
                    out.push(json!({
                        "role": "user",
                        "content": content,
                    }));
                } else {
                    let text = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    out.push(json!({
                        "role": "user",
                        "content": [{ "type": "input_text", "text": text }]
                    }));
                }
            }
            "assistant" => {
                if let Some(text) = msg.get("content").and_then(|v| v.as_str())
                    && !text.is_empty()
                {
                    out.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "status": "completed",
                        "id": format!("msg_{idx}"),
                        "content": [{ "type": "output_text", "text": text }]
                    }));
                }

                if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    for (tool_idx, tc) in tool_calls.iter().enumerate() {
                        let call_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("toolcall");
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool");
                        let arguments = tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("{}");
                        out.push(json!({
                            "type": "function_call",
                            "id": format!("fc_{idx}_{tool_idx}"),
                            "call_id": call_id.split('|').next().unwrap_or(call_id),
                            "name": name,
                            "arguments": arguments,
                        }));
                    }
                }
            }
            "tool" => {
                let tool_call_id = msg
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let text = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                out.push(json!({
                    "type": "function_call_output",
                    "call_id": tool_call_id.split('|').next().unwrap_or(tool_call_id),
                    "output": text,
                }));
            }
            _ => {}
        }
    }
    out
}
