mod codex;
mod sse;

use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::retry::with_retry;
use crate::transforms::{convert_messages_for_openai, strip_thinking_blocks};
use crate::{CompletionRequest, Provider, RequestOptions, StreamEvent};

use codex::extract_openai_account_id;
use sse::process_openai_sse;

/// OpenAI-compatible provider (works with OpenAI, Groq, Ollama, etc.)
pub struct OpenAiProvider {
    client: Client,
}

impl Default for OpenAiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAiProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

fn is_github_copilot_request(options: &RequestOptions) -> bool {
    options
        .headers
        .get("OpenAI-Organization")
        .is_some_and(|value| value == "github-copilot")
        || options.base_url.contains("githubcopilot.com")
        || options.base_url.contains("/api/copilot")
}

fn format_github_copilot_error(status: reqwest::StatusCode, body: &str, model: &str) -> String {
    let lower = body.to_ascii_lowercase();
    let mut lines = vec![format!("HTTP {status}: {body}")];

    if status == reqwest::StatusCode::UNAUTHORIZED {
        lines.push(
            "GitHub Copilot authentication appears invalid or expired. Run `/login` and select GitHub Copilot to refresh the GitHub/Copilot session."
                .to_string(),
        );
    }

    if status == reqwest::StatusCode::FORBIDDEN {
        lines.push(
            "GitHub Copilot rejected this request. Your account may not have access to this model or your Copilot plan/enterprise policy may block it."
                .to_string(),
        );
    }

    if lower.contains("model not supported") || lower.contains("unsupported model") {
        lines.push(format!(
            "Copilot reported that model `{model}` is not supported. In pi's provider docs, GitHub recommends enabling the model in VS Code: Copilot Chat → model selector → select model → Enable."
        ));
    }

    if lower.contains("missing required authorization header") {
        lines.push(
            "The Copilot runtime token was not accepted. Re-run `/login` for GitHub Copilot and try again."
                .to_string(),
        );
    }

    lines.push(
        "Use `/session` to inspect saved Copilot authority, login, cached models, and token expiry info."
            .to_string(),
    );
    lines.join("\n")
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
        if let Some(account_id) = extract_openai_account_id(&options.api_key) {
            return self
                .stream_codex_oauth(request, options, account_id, tx)
                .await;
        }

        let url = format!(
            "{}/chat/completions",
            options.base_url.trim_end_matches('/')
        );

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

        let is_groq = options.base_url.contains("groq.com");
        let is_ollama =
            options.base_url.contains("localhost") || options.base_url.contains("127.0.0.1");

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

        if let Some(ref thinking) = request.thinking {
            let effort = match thinking.as_str() {
                "low" | "minimal" => "low",
                "medium" => "medium",
                "high" | "xhigh" => "high",
                _ => "medium",
            };
            body["reasoning_effort"] = json!(effort);
        }

        let is_copilot = is_github_copilot_request(&options);
        let model_name = request.model.clone();

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
                    .header("Authorization", format!("Bearer {}", options.api_key))
                    .header("Content-Type", "application/json");
                for (k, v) in &options.headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                let body_clone = body.clone();
                let model_name = model_name.clone();
                async move {
                    let resp = r
                        .json(&body_clone)
                        .send()
                        .await
                        .map_err(|e| BbError::Provider(format!("Request failed: {e}")))?;
                    if !resp.status().is_success() {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        let message = if is_copilot {
                            format_github_copilot_error(status, &body, &model_name)
                        } else {
                            format!("HTTP {status}: {body}")
                        };
                        return Err(BbError::Provider(message));
                    }
                    Ok(resp)
                }
            },
        )
        .await?;

        use futures::StreamExt;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut tool_calls: Vec<(String, String, String)> = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            if options.cancel.is_cancelled() {
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }

            let chunk =
                chunk_result.map_err(|e| BbError::Provider(format!("Stream error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if data == "[DONE]" {
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
