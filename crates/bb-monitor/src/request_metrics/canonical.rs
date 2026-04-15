use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

use super::tracker::RequestMetricsSnapshot;

pub fn canonical_json_from_serializable<T: Serialize>(value: &T) -> Result<String> {
    let json = serde_json::to_value(value)?;
    canonical_json_from_value(&json)
}

pub fn canonical_json_from_value(value: &Value) -> Result<String> {
    let mut out = String::new();
    write_canonical_json(value, &mut out)?;
    Ok(out)
}

pub fn canonical_cacheable_prompt(snapshot: &RequestMetricsSnapshot) -> Result<String> {
    let tool_defs = snapshot.combined_tool_definitions();
    canonical_json_from_value(&serde_json::json!([
        {"tools": tool_defs},
        {"system": anthropic_system_blocks(&snapshot.system_prompt)},
        {"messages": anthropic_cacheable_messages(&snapshot.provider_messages)},
    ]))
}

fn write_canonical_json(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&serde_json::to_string(s)?),
        Value::Array(arr) => {
            out.push('[');
            for (idx, item) in arr.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_canonical_json(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            for (idx, key) in keys.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key)?);
                out.push(':');
                write_canonical_json(&map[*key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn anthropic_system_blocks(system_prompt: &str) -> Vec<Value> {
    if system_prompt.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "type": "text",
            "text": system_prompt,
        })]
    }
}

fn anthropic_cacheable_messages(provider_messages: &[Value]) -> Vec<Value> {
    provider_messages
        .iter()
        .filter_map(anthropic_cacheable_message)
        .collect()
}

fn anthropic_cacheable_message(message: &Value) -> Option<Value> {
    let role = message.get("role")?.as_str()?;
    match role {
        "user" => Some(serde_json::json!({
            "role": "user",
            "content": anthropic_normalize_user_content(message.get("content")?),
        })),
        "assistant" => {
            let mut content = Vec::new();

            if let Some(text) = message.get("content").and_then(Value::as_str)
                && !text.is_empty()
            {
                content.push(serde_json::json!({ "type": "text", "text": text }));
            }

            if let Some(arr) = message.get("content").and_then(Value::as_array) {
                for block in arr {
                    content.push(block.clone());
                }
            }

            if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    let args_str = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let args: Value =
                        serde_json::from_str(args_str).unwrap_or_else(|_| serde_json::json!({}));
                    content.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.get("id").and_then(Value::as_str).unwrap_or(""),
                        "name": tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .cloned()
                            .unwrap_or(Value::Null),
                        "input": args,
                    }));
                }
            }

            if content.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "role": "assistant", "content": content }))
            }
        }
        "tool" => Some(serde_json::json!({
            "role": "user",
            "content": [serde_json::json!({
                "type": "tool_result",
                "tool_use_id": message.get("tool_call_id").cloned().unwrap_or(Value::Null),
                "content": anthropic_normalize_tool_content(
                    message.get("content").unwrap_or(&Value::Null),
                ),
            })],
        })),
        _ => None,
    }
}

fn anthropic_normalize_user_content(content: &Value) -> Value {
    match content {
        Value::String(text) => serde_json::json!([{ "type": "text", "text": text }]),
        Value::Array(arr) => Value::Array(arr.clone()),
        other => other.clone(),
    }
}

fn anthropic_normalize_tool_content(content: &Value) -> Value {
    match content {
        Value::Array(arr) => Value::Array(arr.clone()),
        Value::Null => Value::String(String::new()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_cacheable_prompt, canonical_json_from_value};
    use crate::request_metrics::RequestMetricsSnapshot;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_object_keys() {
        let value = json!({"b": 2, "a": 1, "c": {"y": 2, "x": 1}});
        let canonical = canonical_json_from_value(&value).expect("canonical");
        assert_eq!(canonical, r#"{"a":1,"b":2,"c":{"x":1,"y":2}}"#);
    }

    #[test]
    fn canonical_cacheable_prompt_tracks_anthropic_message_shape() {
        let snapshot = RequestMetricsSnapshot {
            system_prompt: "system".to_string(),
            provider_messages: vec![
                serde_json::json!({"role": "user", "content": "hello"}),
                serde_json::json!({
                    "role": "assistant",
                    "content": "ok",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "demo",
                            "arguments": "{\"x\":1}"
                        }
                    }]
                }),
                serde_json::json!({
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "done"
                }),
            ],
            tool_definitions: vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "demo",
                    "description": "desc",
                    "parameters": {"type": "object"}
                }
            })],
            extra_tool_definitions: vec![],
            model: "claude-sonnet-4-6".to_string(),
            max_tokens: Some(42),
            stream: true,
            thinking: None,
        };

        let canonical = canonical_cacheable_prompt(&snapshot).expect("canonical");
        assert!(canonical.contains("\"system\":[{\"text\":\"system\",\"type\":\"text\"}]"));
        assert!(canonical.contains("\"tool_use_id\":\"call_1\""));
        assert!(canonical.contains("\"type\":\"tool_use\""));
        assert!(canonical.contains("\"content\":[{\"text\":\"hello\",\"type\":\"text\"}]"));
    }
}
