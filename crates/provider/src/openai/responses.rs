use super::*;
use futures::StreamExt;

fn debug_openai_responses_enabled() -> bool {
    std::env::var("BB_DEBUG_OPENAI_RESPONSES").ok().as_deref() == Some("1")
}

impl OpenAiProvider {
    pub(super) async fn stream_responses_api(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let url = format!("{}/responses", options.base_url.trim_end_matches('/'));
        let image_detail = if request.model.starts_with("gpt-5.4") {
            "original"
        } else {
            "high"
        };
        let mut body = json!({
            "model": request.model,
            "stream": true,
            "instructions": request.system_prompt,
            "input": convert_messages_for_responses(&request.messages, image_detail),
        });

        if let Some(max_tokens) = request.max_tokens {
            body["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(ref thinking) = request.thinking {
            let effort = match thinking.as_str() {
                "low" | "minimal" => "low",
                "medium" => "medium",
                "high" | "xhigh" => "high",
                _ => "medium",
            };
            body["reasoning"] = json!({ "effort": effort });
        }

        if debug_openai_responses_enabled() {
            eprintln!("[bb/openai-responses] POST {url}");
            eprintln!("[bb/openai-responses] body={}", body);
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
                    .header("Authorization", format!("Bearer {}", options.api_key))
                    .header("Content-Type", "application/json")
                    .header("OpenAI-Beta", "responses=experimental");
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
                if debug_openai_responses_enabled() {
                    eprintln!("[bb/openai-responses] event={}", event);
                }

                match event.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "response.output_text.delta" => {
                        if let Some(delta) = event.get("delta").and_then(|v| v.as_str())
                            && !delta.is_empty()
                        {
                            let _ = tx.send(StreamEvent::TextDelta {
                                text: delta.to_string(),
                            });
                        }
                    }
                    "response.reasoning.delta" | "response.reasoning_summary_text.delta" => {
                        if let Some(delta) = event.get("delta").and_then(|v| v.as_str())
                            && !delta.is_empty()
                        {
                            let _ = tx.send(StreamEvent::ThinkingDelta {
                                text: delta.to_string(),
                            });
                        }
                    }
                    "response.completed" => {
                        if let Some(usage) = event.get("response").and_then(|r| r.get("usage")) {
                            let input = usage
                                .get("input_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let output = usage
                                .get("output_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let cached = usage
                                .get("input_tokens_details")
                                .and_then(|d| d.get("cached_tokens"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let _ = tx.send(StreamEvent::Usage(crate::UsageInfo {
                                input_tokens: input.saturating_sub(cached),
                                output_tokens: output,
                                cache_read_tokens: cached,
                                cache_write_tokens: 0,
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

fn convert_messages_for_responses(messages: &[Value], image_detail: &str) -> Vec<Value> {
    let mut out = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        match role {
            "user" => {
                if let Some(arr) = msg.get("content").and_then(|v| v.as_array()) {
                    let mut content = Vec::new();
                    for block in arr {
                        match block.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                            "text" => {
                                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                    content.push(json!({ "type": "input_text", "text": text }));
                                }
                            }
                            "image" => {
                                let media_type = block
                                    .get("source")
                                    .and_then(|s| s.get("media_type"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("image/png");
                                let data = block
                                    .get("source")
                                    .and_then(|s| s.get("data"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                content.push(json!({
                                    "type": "input_image",
                                    "image_url": format!("data:{media_type};base64,{data}"),
                                    "detail": image_detail
                                }));
                            }
                            _ => {}
                        }
                    }
                    out.push(json!({ "role": "user", "content": content }));
                } else if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                    out.push(json!({
                        "role": "user",
                        "content": [{ "type": "input_text", "text": text }]
                    }));
                }
            }
            "assistant" => {
                if let Some(text) = msg.get("content").and_then(|v| v.as_str())
                    && !text.is_empty()
                {
                    out.push(json!({
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": text }]
                    }));
                }
            }
            "tool" => {
                if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                    out.push(json!({
                        "type": "function_call_output",
                        "call_id": msg.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or(""),
                        "output": text,
                    }));
                }
            }
            _ => {}
        }
    }

    out
}
