use super::*;

/// Convert OpenAI-style messages to Google Generative AI format.
pub fn convert_messages_google(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|msg| {
            let role = msg["role"].as_str()?;
            match role {
                "user" => {
                    if let Some(arr) = msg["content"].as_array() {
                        let parts: Vec<Value> = arr
                            .iter()
                            .filter_map(|block| {
                                let btype = block["type"].as_str().unwrap_or("");
                                match btype {
                                    "text" => {
                                        let text = block["text"].as_str().unwrap_or("");
                                        if text.is_empty() {
                                            None
                                        } else {
                                            Some(json!({ "text": text }))
                                        }
                                    }
                                    "image" => {
                                        let media_type = block["source"]["media_type"]
                                            .as_str()
                                            .unwrap_or("image/png");
                                        let data = block["source"]["data"].as_str().unwrap_or("");
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
                        let text = msg["content"].as_str().unwrap_or("");
                        Some(json!({
                            "role": "user",
                            "parts": [{ "text": text }]
                        }))
                    }
                }
                "assistant" => {
                    let mut parts = Vec::new();

                    if let Some(text) = msg["content"].as_str()
                        && !text.is_empty()
                    {
                        parts.push(json!({ "text": text }));
                    }

                    if let Some(tool_calls) = msg["tool_calls"].as_array() {
                        for tc in tool_calls {
                            let name = tc["function"]["name"].as_str().unwrap_or("");
                            let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                            let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));
                            parts.push(json!({
                                "functionCall": {
                                    "name": name,
                                    "args": args
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
                    let name = msg["name"]
                        .as_str()
                        .or_else(|| msg["tool_call_id"].as_str())
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

fn flatten_tool_content_for_google(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for block in arr {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(text) = block["text"].as_str()
                        && !text.is_empty()
                    {
                        parts.push(text.to_string());
                    }
                }
                Some("image") => {
                    let mime = block["source"]["media_type"]
                        .as_str()
                        .unwrap_or("image/unknown");
                    parts.push(format!("[tool returned image result: {mime}]"));
                }
                _ => {}
            }
        }
        return parts.join("\n");
    }
    String::new()
}

/// Convert OpenAI-style tool definitions to Google functionDeclarations format.
pub fn convert_tools_google(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let func = t.get("function")?;
            let name = func.get("name")?;
            let description = func.get("description").cloned().unwrap_or(json!(""));
            let parameters = func.get("parameters").cloned().unwrap_or(json!({}));
            let google_params = convert_schema_to_google(&parameters);

            Some(json!({
                "name": name,
                "description": description,
                "parameters": google_params
            }))
        })
        .collect()
}

fn convert_schema_to_google(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, value) in map {
                if key == "type" {
                    if let Some(t) = value.as_str() {
                        result.insert(key.clone(), Value::String(t.to_uppercase()));
                    } else {
                        result.insert(key.clone(), value.clone());
                    }
                } else if key == "properties" {
                    if let Value::Object(props) = value {
                        let mut new_props = serde_json::Map::new();
                        for (pk, pv) in props {
                            new_props.insert(pk.clone(), convert_schema_to_google(pv));
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
