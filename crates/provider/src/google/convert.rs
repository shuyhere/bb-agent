use super::*;

fn parse_tool_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| json!({ "_raw": arguments }))
}

fn flatten_tool_content_for_google(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for block in arr {
            match block.get("type").and_then(|value| value.as_str()) {
                Some("text") => {
                    if let Some(text) = block
                        .get("text")
                        .and_then(|value| value.as_str())
                        .filter(|text| !text.is_empty())
                    {
                        parts.push(text.to_string());
                    }
                }
                Some("image") => {
                    let mime = block
                        .get("source")
                        .and_then(|source| source.get("media_type"))
                        .and_then(|value| value.as_str())
                        .unwrap_or("image/unknown");
                    parts.push(format!("[tool returned image result: {mime}]"));
                }
                _ => {}
            }
        }
        return parts.join("\n");
    }
    content.to_string()
}

/// Convert OpenAI-style messages to Google Generative AI format.
pub fn convert_messages_google(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|msg| {
            let role = msg.get("role").and_then(|value| value.as_str())?;
            match role {
                "user" => {
                    if let Some(arr) = msg.get("content")?.as_array() {
                        let parts: Vec<Value> = arr
                            .iter()
                            .filter_map(|block| {
                                match block
                                    .get("type")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or("")
                                {
                                    "text" => {
                                        let text = block
                                            .get("text")
                                            .and_then(|value| value.as_str())
                                            .unwrap_or("");
                                        if text.is_empty() {
                                            None
                                        } else {
                                            Some(json!({ "text": text }))
                                        }
                                    }
                                    "image" => {
                                        let media_type = block
                                            .get("source")
                                            .and_then(|source| source.get("media_type"))
                                            .and_then(|value| value.as_str())
                                            .unwrap_or("image/png");
                                        let data = block
                                            .get("source")
                                            .and_then(|source| source.get("data"))
                                            .and_then(|value| value.as_str())
                                            .unwrap_or("");
                                        Some(json!({
                                            "inlineData": {
                                                "mimeType": media_type,
                                                "data": data
                                            }
                                        }))
                                    }
                                    _ => Some(json!({ "text": block.to_string() })),
                                }
                            })
                            .collect();
                        if parts.is_empty() {
                            None
                        } else {
                            Some(json!({
                                "role": "user",
                                "parts": parts
                            }))
                        }
                    } else {
                        let text = msg
                            .get("content")
                            .and_then(|value| value.as_str())
                            .unwrap_or("");
                        Some(json!({
                            "role": "user",
                            "parts": [{ "text": text }]
                        }))
                    }
                }
                "assistant" => {
                    let mut parts = Vec::new();

                    if let Some(text) = msg.get("content").and_then(|value| value.as_str())
                        && !text.is_empty()
                    {
                        parts.push(json!({ "text": text }));
                    }

                    if let Some(tool_calls) =
                        msg.get("tool_calls").and_then(|value| value.as_array())
                    {
                        for tool_call in tool_calls {
                            let Some(name) = tool_call
                                .get("function")
                                .and_then(|function| function.get("name"))
                                .and_then(|value| value.as_str())
                                .filter(|name| !name.is_empty())
                            else {
                                continue;
                            };
                            let args_str = tool_call
                                .get("function")
                                .and_then(|function| function.get("arguments"))
                                .and_then(|value| value.as_str())
                                .unwrap_or("{}");
                            parts.push(json!({
                                "functionCall": {
                                    "name": name,
                                    "args": parse_tool_arguments(args_str)
                                }
                            }));
                        }
                    }

                    if parts.is_empty() {
                        return None;
                    }

                    Some(json!({
                        "role": "model",
                        "parts": parts
                    }))
                }
                "tool" => {
                    let name = msg
                        .get("name")
                        .and_then(|value| value.as_str())
                        .or_else(|| msg.get("tool_call_id").and_then(|value| value.as_str()))
                        .unwrap_or("unknown");
                    let content = flatten_tool_content_for_google(&msg["content"]);
                    Some(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": name,
                                "response": { "content": content }
                            }
                        }]
                    }))
                }
                "system" => None,
                _ => None,
            }
        })
        .collect()
}

/// Convert OpenAI-style tool definitions to Google functionDeclarations format.
pub fn convert_tools_google(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let func = tool.get("function")?;
            let description = func
                .get("description")
                .cloned()
                .unwrap_or_else(|| json!(""));
            let parameters = func
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object" }));
            let google_params = convert_schema_to_google(&parameters);

            Some(json!({
                "name": func.get("name")?,
                "description": description,
                "parameters": google_params
            }))
        })
        .collect()
}

fn convert_schema_to_google(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut result = serde_json::Map::with_capacity(map.len());
            for (key, value) in map {
                if key == "type" {
                    if let Some(ty) = value.as_str() {
                        result.insert(key.clone(), Value::String(ty.to_uppercase()));
                    } else {
                        result.insert(key.clone(), value.clone());
                    }
                } else if key == "properties" {
                    if let Value::Object(props) = value {
                        let mut new_props = serde_json::Map::with_capacity(props.len());
                        for (prop_key, prop_value) in props {
                            new_props
                                .insert(prop_key.clone(), convert_schema_to_google(prop_value));
                        }
                        result.insert(key.clone(), Value::Object(new_props));
                    } else {
                        result.insert(key.clone(), value.clone());
                    }
                } else if key == "items" {
                    result.insert(key.clone(), convert_schema_to_google(value));
                } else {
                    result.insert(key.clone(), value.clone());
                }
            }
            Value::Object(result)
        }
        other => other.clone(),
    }
}
