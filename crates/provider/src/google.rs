use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{CompletionRequest, Provider, RequestOptions, StreamEvent, UsageInfo};

/// Google Generative AI (Gemini) provider.
pub struct GoogleProvider {
    client: Client,
}

impl GoogleProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    fn name(&self) -> &str {
        "google"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.stream(request, options, tx).await?;

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        Ok(events)
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?key={}&alt=sse",
            options.base_url.trim_end_matches('/'),
            request.model,
            options.api_key,
        );

        let contents = convert_messages_google(&request.messages);
        let tools = convert_tools_google(&request.tools);

        let mut body = json!({
            "contents": contents,
            "generationConfig": {
                "maxOutputTokens": request.max_tokens.unwrap_or(16384),
            }
        });

        if !request.system_prompt.is_empty() {
            body["systemInstruction"] = json!({
                "parts": [{ "text": request.system_prompt }]
            });
        }

        if !tools.is_empty() {
            body["tools"] = json!([{ "functionDeclarations": tools }]);
        }

        let mut req = self.client
            .post(&url)
            .header("content-type", "application/json");

        for (k, v) in &options.headers {
            req = req.header(k.as_str(), v.as_str());
        }

        let response = req
            .json(&body)
            .send()
            .await
            .map_err(|e| BbError::Provider(format!("Request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(BbError::Provider(format!("HTTP {status}: {body}")));
        }

        // Parse SSE stream
        let bytes_stream = response.bytes_stream();
        use futures::StreamExt;
        let mut stream = bytes_stream;
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            if options.cancel.is_cancelled() {
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }

            let chunk = chunk_result
                .map_err(|e| BbError::Provider(format!("Stream error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE lines
            while let Some(pos) = buffer.find("\n\n") {
                let block = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                for line in block.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            let _ = tx.send(StreamEvent::Done);
                            return Ok(());
                        }
                        if let Ok(event) = serde_json::from_str::<Value>(data) {
                            process_google_event(&event, &tx);
                        }
                    }
                }
            }
        }

        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

fn process_google_event(event: &Value, tx: &mpsc::UnboundedSender<StreamEvent>) {
    // Extract usage metadata
    if let Some(usage) = event.get("usageMetadata") {
        let input = usage["promptTokenCount"].as_u64().unwrap_or(0);
        let output = usage["candidatesTokenCount"].as_u64().unwrap_or(0);
        let _ = tx.send(StreamEvent::Usage(UsageInfo {
            input_tokens: input,
            output_tokens: output,
        }));
    }

    // Extract candidate parts
    let candidates = match event.get("candidates").and_then(|c| c.as_array()) {
        Some(c) => c,
        None => return,
    };

    for candidate in candidates {
        let parts = match candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
        {
            Some(p) => p,
            None => continue,
        };

        for part in parts {
            // Text delta
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                let _ = tx.send(StreamEvent::TextDelta {
                    text: text.to_string(),
                });
            }

            // Function call
            if let Some(fc) = part.get("functionCall") {
                let name = fc["name"].as_str().unwrap_or("").to_string();
                let args = fc.get("args").cloned().unwrap_or(json!({}));
                let id = format!("call_{}", name);
                let _ = tx.send(StreamEvent::ToolCallStart {
                    id: id.clone(),
                    name,
                });
                let _ = tx.send(StreamEvent::ToolCallDelta {
                    id: id.clone(),
                    arguments_delta: args.to_string(),
                });
                let _ = tx.send(StreamEvent::ToolCallEnd { id });
            }
        }
    }
}

/// Convert OpenAI-style messages to Google Generative AI format.
pub fn convert_messages_google(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|msg| {
            let role = msg["role"].as_str()?;
            match role {
                "user" => {
                    let text = msg["content"].as_str().unwrap_or("");
                    Some(json!({
                        "role": "user",
                        "parts": [{ "text": text }]
                    }))
                }
                "assistant" => {
                    let mut parts = Vec::new();

                    // Text content
                    if let Some(text) = msg["content"].as_str() {
                        if !text.is_empty() {
                            parts.push(json!({ "text": text }));
                        }
                    }

                    // Tool calls → functionCall parts
                    if let Some(tool_calls) = msg["tool_calls"].as_array() {
                        for tc in tool_calls {
                            let name = tc["function"]["name"].as_str().unwrap_or("");
                            let args_str = tc["function"]["arguments"].as_str().unwrap_or("{}");
                            let args: Value =
                                serde_json::from_str(args_str).unwrap_or(json!({}));
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
                    let content = msg["content"].as_str().unwrap_or("");
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
                "system" => None, // handled via systemInstruction
                _ => None,
            }
        })
        .collect()
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

            // Convert JSON Schema types to Google's uppercase format
            let google_params = convert_schema_to_google(&parameters);

            Some(json!({
                "name": name,
                "description": description,
                "parameters": google_params
            }))
        })
        .collect()
}

/// Convert JSON Schema types (lowercase) to Google's format (uppercase).
fn convert_schema_to_google(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut result = serde_json::Map::new();
            for (key, value) in map {
                if key == "type" {
                    if let Some(t) = value.as_str() {
                        result.insert(
                            key.clone(),
                            Value::String(t.to_uppercase()),
                        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_convert_user_message() {
        let messages = vec![json!({
            "role": "user",
            "content": "Hello"
        })];
        let result = convert_messages_google(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[0]["parts"][0]["text"], "Hello");
    }

    #[test]
    fn test_convert_assistant_message() {
        let messages = vec![json!({
            "role": "assistant",
            "content": "Hi there!"
        })];
        let result = convert_messages_google(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "model");
        assert_eq!(result[0]["parts"][0]["text"], "Hi there!");
    }

    #[test]
    fn test_convert_assistant_with_tool_calls() {
        let messages = vec![json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_1",
                "function": {
                    "name": "read",
                    "arguments": "{\"path\":\"foo.rs\"}"
                }
            }]
        })];
        let result = convert_messages_google(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "model");
        let fc = &result[0]["parts"][0]["functionCall"];
        assert_eq!(fc["name"], "read");
        assert_eq!(fc["args"]["path"], "foo.rs");
    }

    #[test]
    fn test_convert_tool_result() {
        let messages = vec![json!({
            "role": "tool",
            "name": "read",
            "tool_call_id": "call_1",
            "content": "file contents here"
        })];
        let result = convert_messages_google(&messages);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        let fr = &result[0]["parts"][0]["functionResponse"];
        assert_eq!(fr["name"], "read");
        assert_eq!(fr["response"]["content"], "file contents here");
    }

    #[test]
    fn test_system_message_filtered() {
        let messages = vec![json!({
            "role": "system",
            "content": "You are helpful"
        })];
        let result = convert_messages_google(&messages);
        assert!(result.is_empty());
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![json!({
            "function": {
                "name": "read",
                "description": "Read a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    },
                    "required": ["path"]
                }
            }
        })];
        let result = convert_tools_google(&tools);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["name"], "read");
        assert_eq!(result[0]["description"], "Read a file");
        assert_eq!(result[0]["parameters"]["type"], "OBJECT");
        assert_eq!(result[0]["parameters"]["properties"]["path"]["type"], "STRING");
    }

    #[test]
    fn test_convert_tools_empty() {
        let tools: Vec<Value> = vec![];
        let result = convert_tools_google(&tools);
        assert!(result.is_empty());
    }

    #[test]
    fn test_process_google_event_text() {
        let event = json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "Hello world" }],
                    "role": "model"
                }
            }]
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        process_google_event(&event, &tx);
        drop(tx);

        let mut events = Vec::new();
        while let Some(e) = rx.try_recv().ok() {
            events.push(e);
        }
        assert!(events.len() >= 1);
        match &events[0] {
            StreamEvent::TextDelta { text } => assert_eq!(text, "Hello world"),
            other => panic!("Expected TextDelta, got {:?}", other),
        }
    }

    #[test]
    fn test_process_google_event_function_call() {
        let event = json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "read",
                            "args": { "path": "foo.rs" }
                        }
                    }],
                    "role": "model"
                }
            }]
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        process_google_event(&event, &tx);
        drop(tx);

        let mut events = Vec::new();
        while let Some(e) = rx.try_recv().ok() {
            events.push(e);
        }
        assert_eq!(events.len(), 3); // start, delta, end
        match &events[0] {
            StreamEvent::ToolCallStart { name, .. } => assert_eq!(name, "read"),
            other => panic!("Expected ToolCallStart, got {:?}", other),
        }
    }

    #[test]
    fn test_process_google_event_usage() {
        let event = json!({
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 50
            }
        });
        let (tx, mut rx) = mpsc::unbounded_channel();
        process_google_event(&event, &tx);
        drop(tx);

        let mut events = Vec::new();
        while let Some(e) = rx.try_recv().ok() {
            events.push(e);
        }
        assert_eq!(events.len(), 1);
        match &events[0] {
            StreamEvent::Usage(u) => {
                assert_eq!(u.input_tokens, 100);
                assert_eq!(u.output_tokens, 50);
            }
            other => panic!("Expected Usage, got {:?}", other),
        }
    }
}
