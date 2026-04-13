use super::OpenAiProvider;
use super::default_prompt_cache_key;
use bb_core::error::{BbError, BbResult};
use futures::StreamExt;
use serde_json::{Value, json};
use std::collections::HashSet;
use tokio::sync::mpsc;

use crate::CompletionRequest;
use crate::RequestOptions;
use crate::StreamEvent;
use crate::{CacheMetricsSource, UsageInfo};
use crate::retry::with_retry;

pub(super) fn should_use_responses_api(
    request: &CompletionRequest,
    options: &RequestOptions,
) -> bool {
    request.model.starts_with("gpt-5") && is_standard_openai_api_base(&options.base_url)
}

impl OpenAiProvider {
    pub(super) async fn stream_responses_api(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
        messages: Vec<Value>,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let url = format!("{}/responses", options.base_url.trim_end_matches('/'));
        let body = build_responses_request_body(&request, messages);

        let response = with_retry(
            options.max_retries,
            options.retry_base_delay_ms,
            options.max_retry_delay_ms,
            options.cancel.clone(),
            options.retry_callback.clone(),
            || {
                let mut builder = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", options.api_key))
                    .header("Content-Type", "application/json")
                    .header("accept", "text/event-stream");
                for (k, v) in &options.headers {
                    builder = builder.header(k.as_str(), v.as_str());
                }
                let body_clone = body.clone();
                async move {
                    let resp = builder
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
        let mut started_tool_calls = HashSet::new();
        let mut completed_tool_calls = HashSet::new();

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
                if process_responses_sse(
                    &event,
                    &tx,
                    &mut started_tool_calls,
                    &mut completed_tool_calls,
                ) {
                    return Ok(());
                }
            }
        }

        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}

fn is_standard_openai_api_base(base_url: &str) -> bool {
    let trimmed = base_url.trim_end_matches('/');
    trimmed == "https://api.openai.com/v1" || trimmed == "https://api.openai.com"
}

fn build_responses_request_body(request: &CompletionRequest, messages: Vec<Value>) -> Value {
    let mut body = json!({
        "model": request.model,
        "input": convert_messages_for_responses(&messages),
        "stream": true,
        "store": false,
        "text": { "verbosity": "medium" },
        "prompt_cache_key": default_prompt_cache_key(&request.model),
    });

    if let Some(max_tokens) = request.max_tokens {
        body["max_output_tokens"] = json!(max_tokens);
    }
    if let Some(ref thinking) = request.thinking {
        body["reasoning"] = json!({
            "effort": responses_reasoning_effort(thinking.as_str()),
            "summary": "auto"
        });
    }
    if !request.tools.is_empty() {
        body["tools"] = json!(request.tools);
        body["tool_choice"] = json!("auto");
        body["parallel_tool_calls"] = json!(false);
    }

    body
}

fn responses_reasoning_effort(thinking: &str) -> &'static str {
    match thinking {
        "low" | "minimal" => "low",
        "medium" => "medium",
        "high" | "xhigh" => "high",
        _ => "medium",
    }
}

fn convert_messages_for_responses(messages: &[Value]) -> Vec<Value> {
    let messages = sanitize_messages_for_responses(messages);
    let mut out = Vec::new();

    for (idx, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        match role {
            "assistant" => push_assistant_message(&mut out, idx, msg),
            "tool" => push_tool_result_message(&mut out, msg),
            "user" | "system" => push_user_or_system_message(&mut out, role, msg),
            _ => {}
        }
    }

    out
}

fn sanitize_messages_for_responses(messages: &[Value]) -> Vec<Value> {
    let mut result = Vec::new();
    let mut pending_tool_calls: HashSet<String> = HashSet::new();

    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        match role {
            "assistant" => {
                pending_tool_calls.clear();
                if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                            pending_tool_calls
                                .insert(id.split('|').next().unwrap_or(id).to_string());
                        }
                    }
                }
                result.push(msg.clone());
            }
            "tool" => {
                let tool_call_id = msg
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let call_id = tool_call_id.split('|').next().unwrap_or(tool_call_id);
                if pending_tool_calls.remove(call_id) {
                    result.push(msg.clone());
                }
            }
            "user" | "system" => {
                pending_tool_calls.clear();
                result.push(msg.clone());
            }
            _ => result.push(msg.clone()),
        }
    }

    result
}

fn push_user_or_system_message(out: &mut Vec<Value>, role: &str, msg: &Value) {
    match msg.get("content") {
        Some(Value::String(text)) => out.push(json!({
            "role": role,
            "content": [{ "type": "input_text", "text": text }]
        })),
        Some(Value::Array(parts)) => {
            let mut content = Vec::new();
            for part in parts {
                match part.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "text" => {
                        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                            content.push(json!({ "type": "input_text", "text": text }));
                        }
                    }
                    "image" => {
                        let media_type = part
                            .get("source")
                            .and_then(|s| s.get("media_type"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("image/png");
                        let data = part
                            .get("source")
                            .and_then(|s| s.get("data"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        content.push(json!({
                            "type": "input_image",
                            "image_url": format!("data:{media_type};base64,{data}"),
                            "detail": "high",
                        }));
                    }
                    _ => {}
                }
            }
            out.push(json!({ "role": role, "content": content }));
        }
        _ => out.push(json!({ "role": role, "content": [] })),
    }
}

fn push_assistant_message(out: &mut Vec<Value>, idx: usize, msg: &Value) {
    if let Some(text) = msg.get("content").and_then(|v| v.as_str())
        && !text.is_empty()
    {
        out.push(json!({
            "type": "message",
            "role": "assistant",
            "status": "completed",
            "id": format!("msg_{idx}"),
            "content": [{ "type": "output_text", "text": text }],
        }));
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

fn push_tool_result_message(out: &mut Vec<Value>, msg: &Value) {
    let tool_call_id = msg
        .get("tool_call_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let text = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
    out.push(json!({
        "type": "function_call_output",
        "call_id": tool_call_id.split('|').next().unwrap_or(tool_call_id),
        "output": text,
    }));
}

fn process_responses_sse(
    event: &Value,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    started_tool_calls: &mut HashSet<String>,
    completed_tool_calls: &mut HashSet<String>,
) -> bool {
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
        "response.reasoning_summary_text.delta" => {
            if let Some(delta) = event.get("delta").and_then(|v| v.as_str())
                && !delta.is_empty()
            {
                let _ = tx.send(StreamEvent::ThinkingDelta {
                    text: delta.to_string(),
                });
            }
        }
        "response.output_item.added" => {
            maybe_send_tool_call_start(event, tx, started_tool_calls);
        }
        "response.output_item.done" => {
            maybe_send_tool_call_done(event, tx, started_tool_calls, completed_tool_calls);
        }
        "response.completed" => {
            send_responses_usage(event, tx);
            let _ = tx.send(StreamEvent::Done);
            return true;
        }
        _ => {}
    }

    false
}

fn maybe_send_tool_call_start(
    event: &Value,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    started_tool_calls: &mut HashSet<String>,
) {
    let Some(item) = event.get("item") else {
        return;
    };
    if item.get("type").and_then(|v| v.as_str()) != Some("function_call") {
        return;
    }

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

fn maybe_send_tool_call_done(
    event: &Value,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    started_tool_calls: &mut HashSet<String>,
    completed_tool_calls: &mut HashSet<String>,
) {
    let Some(item) = event.get("item") else {
        return;
    };
    if item.get("type").and_then(|v| v.as_str()) != Some("function_call") {
        return;
    }

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

fn send_responses_usage(event: &Value, tx: &mpsc::UnboundedSender<StreamEvent>) {
    let Some(usage) = event.get("response").and_then(|r| r.get("usage")) else {
        return;
    };

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
        cache_metrics_source: CacheMetricsSource::Official,
    }));
}
