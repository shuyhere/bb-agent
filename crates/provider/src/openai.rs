use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{CompletionRequest, Provider, RequestOptions, StreamEvent, UsageInfo};
use crate::retry::with_retry;

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
        let url = format!(
            "{}/chat/completions",
            options.base_url.trim_end_matches('/')
        );

        let mut messages = Vec::new();
        if !request.system_prompt.is_empty() {
            messages.push(json!({"role": "system", "content": request.system_prompt}));
        }
        messages.extend(request.messages.iter().cloned());

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
