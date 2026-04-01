use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{CompletionRequest, Provider, RequestOptions, StreamEvent, UsageInfo};
use crate::retry::with_retry;
use crate::transforms::convert_messages_for_anthropic;

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    client: Client,
}

impl AnthropicProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
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
            "{}/v1/messages",
            options.base_url.trim_end_matches('/')
        );

        // Build messages in Anthropic format using shared transform layer
        let messages = convert_messages_for_anthropic(&request.messages);

        // Build tools in Anthropic format
        let tools: Vec<Value> = request.tools.iter().map(|t| {
            let func = &t["function"];
            json!({
                "name": func["name"],
                "description": func["description"],
                "input_schema": func["parameters"],
            })
        }).collect();

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(16384),
            "stream": true,
        });

        if !request.system_prompt.is_empty() {
            body["system"] = json!(request.system_prompt);
        }

        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        if let Some(ref thinking) = request.thinking {
            let budget = match thinking.as_str() {
                "minimal" => 1024,
                "low" => 2048,
                "medium" => 8192,
                "high" => 16384,
                "xhigh" => 32768,
                _ => 8192,
            };
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
            // When thinking is enabled, Anthropic requires max_tokens to be higher
            if request.max_tokens.unwrap_or(0) < (budget as u32 + 4096) {
                body["max_tokens"] = json!(budget + 4096);
            }
        }

        let response = with_retry(3, || {
            let mut r = self.client
                .post(&url)
                .header("x-api-key", &options.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json");
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
                            process_sse_event(&event, &tx);
                        }
                    }
                }
            }
        }

        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

/// Track block index → tool_use ID for correlating deltas.
static BLOCK_ID_MAP: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<u64, String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn process_sse_event(event: &Value, tx: &mpsc::UnboundedSender<StreamEvent>) {
    let event_type = event["type"].as_str().unwrap_or("");

    match event_type {
        "message_start" => {
            // Clear block ID map for new message
            BLOCK_ID_MAP.lock().unwrap().clear();
            if let Some(usage) = event.get("message").and_then(|m| m.get("usage")) {
                let _ = tx.send(StreamEvent::Usage(UsageInfo {
                    input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
                }));
            }
        }
        "content_block_start" => {
            if let Some(block) = event.get("content_block") {
                let block_type = block["type"].as_str().unwrap_or("");
                match block_type {
                    "tool_use" => {
                        let id = block["id"].as_str().unwrap_or("").to_string();
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        let index = event["index"].as_u64().unwrap_or(0);
                        // Track index → id mapping for delta correlation
                        BLOCK_ID_MAP.lock().unwrap().insert(index, id.clone());
                        let _ = tx.send(StreamEvent::ToolCallStart { id, name });
                    }
                    // text and thinking blocks emit via deltas
                    _ => {}
                }
            }
        }
        "content_block_delta" => {
            if let Some(delta) = event.get("delta") {
                let delta_type = delta["type"].as_str().unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(text) = delta["text"].as_str() {
                            let _ = tx.send(StreamEvent::TextDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta["thinking"].as_str() {
                            let _ = tx.send(StreamEvent::ThinkingDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                    "input_json_delta" => {
                        if let Some(json_str) = delta["partial_json"].as_str() {
                            let index = event["index"].as_u64().unwrap_or(0);
                            // Look up the real tool_use ID from the block index
                            let id = BLOCK_ID_MAP
                                .lock()
                                .unwrap()
                                .get(&index)
                                .cloned()
                                .unwrap_or_else(|| format!("block_{index}"));
                            let _ = tx.send(StreamEvent::ToolCallDelta {
                                id,
                                arguments_delta: json_str.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        "content_block_stop" => {
            let index = event["index"].as_u64().unwrap_or(0);
            if let Some(id) = BLOCK_ID_MAP.lock().unwrap().get(&index).cloned() {
                let _ = tx.send(StreamEvent::ToolCallEnd { id });
            }
        }
        "message_delta" => {
            if let Some(usage) = event.get("usage") {
                let _ = tx.send(StreamEvent::Usage(UsageInfo {
                    input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
                }));
            }
        }
        "message_stop" => {
            let _ = tx.send(StreamEvent::Done);
        }
        _ => {}
    }
}

// Message conversion is now handled by crate::transforms::convert_messages_for_anthropic
