mod events;

use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::retry::with_retry;
use crate::transforms::convert_messages_for_anthropic;
use crate::{CompletionRequest, Provider, ProviderAuthMode, RequestOptions, StreamEvent};

use bb_core::types::CacheMetricsSource;
use events::process_sse_event;

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    client: Client,
}

impl Default for AnthropicProvider {
    fn default() -> Self {
        Self::new()
    }
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
        let url = format!("{}/v1/messages", options.base_url.trim_end_matches('/'));
        let is_oauth = matches!(options.auth_mode, ProviderAuthMode::OAuth);

        let mut messages = convert_messages_for_anthropic(&request.messages);
        apply_cache_control_to_last_user_message(&mut messages);

        let mut tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                let func = &t["function"];
                json!({
                    "name": func["name"],
                    "description": func["description"],
                    "input_schema": func["parameters"],
                })
            })
            .collect();
        tools.extend(request.extra_tool_schemas.iter().cloned());

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(16384),
            "stream": true,
        });

        if is_oauth {
            let mut system_blocks = vec![system_text_block(
                "You are Claude Code, Anthropic's official CLI for Claude.",
            )];
            if !request.system_prompt.is_empty() {
                system_blocks.push(system_text_block(&request.system_prompt));
            }
            body["system"] = json!(system_blocks);
        } else if !request.system_prompt.is_empty() {
            body["system"] = json!([system_text_block(&request.system_prompt)]);
        }

        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        if let Some(ref thinking) = request.thinking {
            if supports_adaptive_thinking(&request.model) {
                let effort = match thinking.as_str() {
                    "minimal" | "low" => "low",
                    "medium" => "medium",
                    "high" => "high",
                    "xhigh" => {
                        if request.model.contains("opus-4-6") {
                            "max"
                        } else {
                            "high"
                        }
                    }
                    _ => "medium",
                };
                body["thinking"] = json!({ "type": "adaptive" });
                body["output_config"] = json!({ "effort": effort });
            } else {
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
                if request.max_tokens.unwrap_or(0) < (budget as u32 + 4096) {
                    body["max_tokens"] = json!(budget + 4096);
                }
            }
        }

        let response = with_retry(
            options.max_retries,
            options.retry_base_delay_ms,
            options.max_retry_delay_ms,
            options.cancel.clone(),
            options.retry_callback.clone(),
            || {
                let mut r = self
                    .client
                    .post(&url)
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .header("accept", "application/json")
                    .header("anthropic-dangerous-direct-browser-access", "true");

                if is_oauth {
                    r = r
                        .header("Authorization", format!("Bearer {}", options.api_key))
                        .header(
                            "anthropic-beta",
                            "claude-code-20250219,oauth-2025-04-20,fine-grained-tool-streaming-2025-05-14",
                        )
                        .header("user-agent", "claude-cli/2.1.75")
                        .header("x-app", "cli");
                } else {
                    r = r
                        .header("x-api-key", &options.api_key)
                        .header("anthropic-beta", "fine-grained-tool-streaming-2025-05-14");
                }

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
            },
        )
        .await?;

        use futures::StreamExt;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            if options.cancel.is_cancelled() {
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }

            let chunk =
                chunk_result.map_err(|e| BbError::Provider(format!("Stream error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

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
                            process_sse_event(
                                &event,
                                &tx,
                                cache_metrics_source_for_auth_mode(&options.auth_mode),
                            );
                        }
                    }
                }
            }
        }

        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

fn cache_metrics_source_for_auth_mode(auth_mode: &ProviderAuthMode) -> CacheMetricsSource {
    match auth_mode {
        ProviderAuthMode::ApiKey => CacheMetricsSource::Official,
        ProviderAuthMode::OAuth => CacheMetricsSource::Estimated,
    }
}

fn anthropic_cache_control() -> Value {
    json!({ "type": "ephemeral" })
}

fn system_text_block(text: &str) -> Value {
    json!({
        "type": "text",
        "text": text,
        "cache_control": anthropic_cache_control(),
    })
}

fn apply_cache_control_to_last_user_message(messages: &mut [Value]) {
    let Some(last_message) = messages
        .iter_mut()
        .rev()
        .find(|message| message.get("role").and_then(|value| value.as_str()) == Some("user"))
    else {
        return;
    };

    match last_message.get_mut("content") {
        Some(Value::Array(parts)) => {
            if let Some(Value::Object(last_part)) = parts.last_mut() {
                let block_type = last_part.get("type").and_then(|value| value.as_str());
                if matches!(block_type, Some("text" | "image" | "tool_result")) {
                    last_part.insert("cache_control".to_string(), anthropic_cache_control());
                }
            }
        }
        Some(Value::String(text)) => {
            let converted = json!([{
                "type": "text",
                "text": text.clone(),
                "cache_control": anthropic_cache_control(),
            }]);
            last_message["content"] = converted;
        }
        _ => {}
    }
}

fn supports_adaptive_thinking(model: &str) -> bool {
    model.contains("claude-opus-4-6") || model.contains("claude-sonnet-4-6")
}

#[cfg(test)]
mod tests {
    use super::{
        CacheMetricsSource, ProviderAuthMode, apply_cache_control_to_last_user_message,
        cache_metrics_source_for_auth_mode, system_text_block,
    };
    use serde_json::json;

    #[test]
    fn api_key_uses_official_cache_metrics_and_oauth_uses_estimates() {
        assert_eq!(
            cache_metrics_source_for_auth_mode(&ProviderAuthMode::ApiKey),
            CacheMetricsSource::Official
        );
        assert_eq!(
            cache_metrics_source_for_auth_mode(&ProviderAuthMode::OAuth),
            CacheMetricsSource::Estimated
        );
    }

    #[test]
    fn system_blocks_include_ephemeral_cache_control() {
        let block = system_text_block("system prompt");
        assert_eq!(block["type"], "text");
        assert_eq!(block["text"], "system prompt");
        assert_eq!(block["cache_control"], json!({ "type": "ephemeral" }));
    }

    #[test]
    fn adds_cache_control_to_last_user_message_text_block() {
        let mut messages = vec![
            json!({"role": "assistant", "content": "previous"}),
            json!({"role": "user", "content": [{"type": "text", "text": "hello"}]}),
        ];

        apply_cache_control_to_last_user_message(&mut messages);

        assert_eq!(
            messages[1]["content"][0]["cache_control"],
            json!({ "type": "ephemeral" })
        );
    }

    #[test]
    fn converts_string_user_message_into_cacheable_text_block() {
        let mut messages = vec![json!({"role": "user", "content": "hello"})];

        apply_cache_control_to_last_user_message(&mut messages);

        assert_eq!(messages[0]["content"][0]["type"], "text");
        assert_eq!(messages[0]["content"][0]["text"], "hello");
        assert_eq!(
            messages[0]["content"][0]["cache_control"],
            json!({ "type": "ephemeral" })
        );
    }
}
