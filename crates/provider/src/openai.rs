use async_trait::async_trait;
use base64::Engine;
use bb_core::error::{BbError, BbResult};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{CompletionRequest, Provider, RequestOptions, StreamEvent, UsageInfo};
use crate::retry::with_retry;
use crate::transforms::{convert_messages_for_openai, strip_thinking_blocks};

/// OpenAI-compatible provider (works with OpenAI, Groq, Ollama, etc.)
pub struct OpenAiProvider {
    client: Client,
}

impl OpenAiProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
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
        // pi compatibility: if the credential is a ChatGPT Codex OAuth JWT,
        // do NOT send it to api.openai.com/v1/chat/completions.
        // pi uses chatgpt.com/backend-api/codex/responses with chatgpt-account-id.
        if let Some(account_id) = extract_openai_account_id(&options.api_key) {
            return self.stream_codex_oauth(request, options, account_id, tx).await;
        }

        let url = format!(
            "{}/chat/completions",
            options.base_url.trim_end_matches('/')
        );

        // Apply message transforms: strip thinking blocks and convert to OpenAI format
        let transformed = strip_thinking_blocks(&request.messages);
        let converted = convert_messages_for_openai(&transformed);

        let mut messages = Vec::new();
        if !request.system_prompt.is_empty() {
            messages.push(json!({"role": "system", "content": request.system_prompt}));
        }
        messages.extend(converted);

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
        });

        // Some providers don't support max_completion_tokens, use max_tokens instead
        let is_groq = options.base_url.contains("groq.com");
        let is_ollama = options.base_url.contains("localhost") || options.base_url.contains("127.0.0.1");

        if let Some(max_tokens) = request.max_tokens {
            if is_groq || is_ollama {
                body["max_tokens"] = json!(max_tokens);
            } else {
                body["max_completion_tokens"] = json!(max_tokens);
            }
        }
        if !request.tools.is_empty() {
            body["tools"] = json!(request.tools);
        }

        // Add reasoning_effort for OpenAI models that support it
        if let Some(ref thinking) = request.thinking {
            let effort = match thinking.as_str() {
                "low" | "minimal" => "low",
                "medium" => "medium",
                "high" | "xhigh" => "high",
                _ => "medium",
            };
            body["reasoning_effort"] = json!(effort);
        }

        let response = with_retry(3, || {
            let mut r = self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", options.api_key))
                .header("Content-Type", "application/json");
            for (k, v) in &options.headers {
                r = r.header(k.as_str(), v.as_str());
            }
            let body_clone = body.clone();
            async move {
                let resp = r
                    .json(&body_clone)
                    .send()
                    .await
                    .map_err(|e| BbError::Provider(format!("Request failed: {e}")))?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(BbError::Provider(format!("HTTP {status}: {body}")));
                }
                Ok(resp)
            }
        }).await?;

        // Parse SSE stream
        use futures::StreamExt;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut tool_calls: Vec<(String, String, String)> = Vec::new(); // (id, name, args)

        while let Some(chunk_result) = stream.next().await {
            if options.cancel.is_cancelled() {
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }

            let chunk = chunk_result
                .map_err(|e| BbError::Provider(format!("Stream error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
                        // Emit any accumulated tool calls
                        for (id, name, args) in &tool_calls {
                            let _ = tx.send(StreamEvent::ToolCallStart {
                                id: id.clone(),
                                name: name.clone(),
                            });
                            let _ = tx.send(StreamEvent::ToolCallDelta {
                                id: id.clone(),
                                arguments_delta: args.clone(),
                            });
                            let _ = tx.send(StreamEvent::ToolCallEnd { id: id.clone() });
                        }
                        let _ = tx.send(StreamEvent::Done);
                        return Ok(());
                    }

                    if let Ok(event) = serde_json::from_str::<Value>(data) {
                        process_openai_sse(&event, &tx, &mut tool_calls);
                    }
                }
            }
        }

        // Final: emit any remaining tool calls
        for (id, name, args) in &tool_calls {
            let _ = tx.send(StreamEvent::ToolCallStart {
                id: id.clone(),
                name: name.clone(),
            });
            let _ = tx.send(StreamEvent::ToolCallDelta {
                id: id.clone(),
                arguments_delta: args.clone(),
            });
            let _ = tx.send(StreamEvent::ToolCallEnd { id: id.clone() });
        }
        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

impl OpenAiProvider {
    async fn stream_codex_oauth(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
        account_id: String,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        use futures::StreamExt;

        let url = resolve_codex_url(&options.base_url);
        let mut body = json!({
            "model": request.model,
            "store": false,
            "stream": true,
            "instructions": request.system_prompt,
            "input": convert_messages_for_codex(&request.messages),
            "text": { "verbosity": "medium" },
            "tool_choice": "auto",
            "parallel_tool_calls": true,
        });

        if !request.tools.is_empty() {
            body["tools"] = json!(convert_tools_for_codex(&request.tools));
        }
        if let Some(ref thinking) = request.thinking {
            let effort = match thinking.as_str() {
                "low" | "minimal" => "low",
                "medium" => "medium",
                "high" | "xhigh" => "high",
                _ => "medium",
            };
            body["reasoning"] = json!({ "effort": effort, "summary": "auto" });
        }

        let response = with_retry(3, || {
            let mut r = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", options.api_key))
                .header("chatgpt-account-id", &account_id)
                .header("OpenAI-Beta", "responses=experimental")
                .header("accept", "text/event-stream")
                .header("content-type", "application/json")
                .header("originator", "pi")
                .header("User-Agent", "bb-agent");
            for (k, v) in &options.headers {
                r = r.header(k.as_str(), v.as_str());
            }
            let body_clone = body.clone();
            async move {
                let resp = r
                    .json(&body_clone)
                    .send()
                    .await
                    .map_err(|e| BbError::Provider(format!("Codex OAuth request failed: {e}")))?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(BbError::Provider(format!("HTTP {status}: {body}")));
                }
                Ok(resp)
            }
        }).await?;

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut current_tool: Option<(String, String)> = None;

        while let Some(chunk_result) = stream.next().await {
            if options.cancel.is_cancelled() {
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }

            let chunk = chunk_result
                .map_err(|e| BbError::Provider(format!("Codex OAuth stream error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }
                let Some(data) = line.strip_prefix("data: ") else {
                    continue;
                };
                if data == "[DONE]" {
                    let _ = tx.send(StreamEvent::Done);
                    return Ok(());
                }
                let Ok(event) = serde_json::from_str::<Value>(data) else {
                    continue;
                };

                match event.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "response.output_item.added" => {
                        if let Some(item) = event.get("item") {
                            if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("toolcall");
                                let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("item");
                                let id = format!("{call_id}|{item_id}");
                                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("tool").to_string();
                                current_tool = Some((id.clone(), name.clone()));
                                let _ = tx.send(StreamEvent::ToolCallStart { id, name });
                            }
                        }
                    }
                    "response.output_text.delta" => {
                        if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                            if !delta.is_empty() {
                                let _ = tx.send(StreamEvent::TextDelta { text: delta.to_string() });
                            }
                        }
                    }
                    "response.reasoning_summary_text.delta" => {
                        if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                            if !delta.is_empty() {
                                let _ = tx.send(StreamEvent::ThinkingDelta { text: delta.to_string() });
                            }
                        }
                    }
                    "response.function_call_arguments.delta" => {
                        if let Some((id, _)) = &current_tool {
                            if let Some(delta) = event.get("delta").and_then(|v| v.as_str()) {
                                let _ = tx.send(StreamEvent::ToolCallDelta {
                                    id: id.clone(),
                                    arguments_delta: delta.to_string(),
                                });
                            }
                        }
                    }
                    "response.output_item.done" => {
                        if let Some(item) = event.get("item") {
                            if item.get("type").and_then(|v| v.as_str()) == Some("function_call") {
                                if let Some((id, _)) = current_tool.take() {
                                    let _ = tx.send(StreamEvent::ToolCallEnd { id });
                                }
                            }
                        }
                    }
                    "response.completed" => {
                        if let Some(usage) = event.get("response").and_then(|r| r.get("usage")) {
                            let cached = usage
                                .get("input_tokens_details")
                                .and_then(|d| d.get("cached_tokens"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            let _ = tx.send(StreamEvent::Usage(UsageInfo {
                                input_tokens: input.saturating_sub(cached),
                                output_tokens: output,
                            }));
                        }
                        let _ = tx.send(StreamEvent::Done);
                        return Ok(());
                    }
                    _ => {}
                }
            }
        }

        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

fn process_openai_sse(
    event: &Value,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    tool_calls: &mut Vec<(String, String, String)>,
) {
    if let Some(choices) = event.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            let delta = &choice["delta"];

            // Text content
            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                if !content.is_empty() {
                    let _ = tx.send(StreamEvent::TextDelta {
                        text: content.to_string(),
                    });
                }
            }

            // Tool calls (accumulated across deltas)
            if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tcs {
                    let index = tc["index"].as_u64().unwrap_or(0) as usize;

                    // Ensure we have enough entries
                    while tool_calls.len() <= index {
                        tool_calls.push((String::new(), String::new(), String::new()));
                    }

                    if let Some(id) = tc["id"].as_str() {
                        tool_calls[index].0 = id.to_string();
                    }
                    if let Some(name) = tc["function"]["name"].as_str() {
                        tool_calls[index].1 = name.to_string();
                    }
                    if let Some(args) = tc["function"]["arguments"].as_str() {
                        tool_calls[index].2.push_str(args);
                    }
                }
            }
        }
    }

    // Usage info
    if let Some(usage) = event.get("usage") {
        let _ = tx.send(StreamEvent::Usage(UsageInfo {
            input_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: usage["completion_tokens"].as_u64().unwrap_or(0),
        }));
    }
}

fn resolve_codex_url(base_url: &str) -> String {
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

fn convert_tools_for_codex(tools: &[Value]) -> Vec<Value> {
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

fn convert_messages_for_codex(messages: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    for (idx, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        match role {
            "user" => {
                let text = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                out.push(json!({
                    "role": "user",
                    "content": [{ "type": "input_text", "text": text }]
                }));
            }
            "assistant" => {
                if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        out.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "status": "completed",
                            "id": format!("msg_{idx}"),
                            "content": [{ "type": "output_text", "text": text }]
                        }));
                    }
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
                let tool_call_id = msg.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
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

fn extract_openai_account_id(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let json: Value = serde_json::from_slice(&decoded).ok()?;
    json.get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
