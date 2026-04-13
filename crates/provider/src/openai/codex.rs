mod auth;
mod request;

#[cfg(test)]
mod request_tests;

use super::*;
use futures::StreamExt;
use std::collections::HashSet;

use crate::{CacheMetricsSource, UsageInfo};

pub(super) use auth::extract_openai_account_id;
use request::{
    codex_reasoning_effort, convert_messages_for_codex, convert_tools_for_codex, resolve_codex_url,
};

impl OpenAiProvider {
    pub(super) async fn stream_codex_oauth(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
        account_id: String,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let url = resolve_codex_url(&options.base_url);
        let mut body = json!({
            "model": request.model,
            "store": false,
            "stream": true,
            "instructions": request.system_prompt,
            "input": convert_messages_for_codex(&request.messages),
            "text": { "verbosity": "medium" },
            "tool_choice": "auto",
            "parallel_tool_calls": false,
        });

        if !request.tools.is_empty() {
            body["tools"] = json!(convert_tools_for_codex(&request.tools));
        }
        if let Some(ref thinking) = request.thinking {
            body["reasoning"] = json!({
                "effort": codex_reasoning_effort(thinking.as_str()),
                "summary": "auto"
            });
        }

        body["prompt_cache_key"] = json!(super::default_prompt_cache_key(&request.model));

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
                    .header("chatgpt-account-id", &account_id)
                    .header("OpenAI-Beta", "responses=experimental")
                    .header("accept", "text/event-stream")
                    .header("content-type", "application/json")
                    .header("originator", "bb")
                    .header("User-Agent", "bb-agent");
                for (k, v) in &options.headers {
                    r = r.header(k.as_str(), v.as_str());
                }
                let body_clone = body.clone();
                async move {
                    let resp = r.json(&body_clone).send().await.map_err(|e| {
                        BbError::Provider(format!("Codex OAuth request failed: {e}"))
                    })?;
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

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut started_tool_calls: HashSet<String> = HashSet::new();
        let mut completed_tool_calls: HashSet<String> = HashSet::new();

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
                        if let Some(item) = event.get("item")
                            && item.get("type").and_then(|v| v.as_str()) == Some("function_call")
                        {
                            let call_id = item
                                .get("call_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("toolcall");
                            let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("item");
                            let id = format!("{call_id}|{item_id}");
                            let name = item
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("tool")
                                .to_string();
                            if started_tool_calls.insert(id.clone()) {
                                let _ = tx.send(StreamEvent::ToolCallStart { id, name });
                            }
                        }
                    }
                    "response.output_text.delta" => {
                        if let Some(delta) = event.get("delta").and_then(|v| v.as_str())
                            && !delta.is_empty()
                        {
                            let _ = tx.send(StreamEvent::TextDelta {
                                text: delta.to_string(),
                            });
                        }
                    }
                    "response.reasoning_summary_text.delta" => {
                        if let Some(delta) = event.get("delta").and_then(|v| v.as_str())
                            && !delta.is_empty()
                        {
                            let _ = tx.send(StreamEvent::ThinkingDelta {
                                text: delta.to_string(),
                            });
                        }
                    }
                    "response.function_call_arguments.delta" => {}
                    "response.output_item.done" => {
                        if let Some(item) = event.get("item")
                            && item.get("type").and_then(|v| v.as_str()) == Some("function_call")
                        {
                            let call_id = item
                                .get("call_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("toolcall");
                            let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or("item");
                            let id = format!("{call_id}|{item_id}");
                            let name = item
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("tool")
                                .to_string();
                            let arguments = item
                                .get("arguments")
                                .and_then(|v| v.as_str())
                                .unwrap_or("{}");

                            if started_tool_calls.insert(id.clone()) {
                                let _ = tx.send(StreamEvent::ToolCallStart {
                                    id: id.clone(),
                                    name,
                                });
                            }
                            let _ = tx.send(StreamEvent::ToolCallDelta {
                                id: id.clone(),
                                arguments_delta: arguments.to_string(),
                            });
                            if completed_tool_calls.insert(id.clone()) {
                                let _ = tx.send(StreamEvent::ToolCallEnd { id });
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
                            let input = usage
                                .get("input_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let output = usage
                                .get("output_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let _ = tx.send(StreamEvent::Usage(UsageInfo {
                                input_tokens: input.saturating_sub(cached),
                                output_tokens: output,
                                cache_read_tokens: cached,
                                cache_write_tokens: 0,
                                cache_metrics_source: CacheMetricsSource::Estimated,
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
