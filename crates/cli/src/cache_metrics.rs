use bb_core::config;
use bb_core::types::AgentMessage;
use bb_provider::{CacheMetricsSource, CollectedResponse, CompletionRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex;
use uuid::Uuid;

static REQUEST_METRICS_FILE_LOCK: LazyLock<StdMutex<()>> = LazyLock::new(|| StdMutex::new(()));

#[derive(Clone, Debug, Default)]
pub(crate) struct RequestMetricsState {
    pub last_request_hash: Option<String>,
    pub last_cacheable_prompt: Option<String>,
    pub context_epoch: u64,
}

pub(crate) type SharedRequestMetricsState = Arc<Mutex<RequestMetricsState>>;

pub(crate) fn new_shared_request_metrics_state() -> SharedRequestMetricsState {
    Arc::new(Mutex::new(RequestMetricsState::default()))
}

pub(crate) async fn hydrate_request_metrics_state_from_messages(
    state: &SharedRequestMetricsState,
    request: &CompletionRequest,
) -> anyhow::Result<()> {
    let canonical_request = canonical_json_from_serializable(request)?;
    let full_request_hash = sha256_hex(canonical_request.as_bytes());
    let cacheable_prompt = canonical_cacheable_prompt_from_request(request)?;

    let mut state_guard = state.lock().await;
    state_guard.last_request_hash = Some(full_request_hash);
    state_guard.last_cacheable_prompt = Some(cacheable_prompt);
    Ok(())
}

pub(crate) async fn hydrate_request_metrics_state_from_session_messages(
    state: &SharedRequestMetricsState,
    system_prompt: &str,
    tool_defs: &[Value],
    session_messages: &[AgentMessage],
    model: &str,
    max_tokens: Option<u32>,
    thinking: Option<&str>,
) -> anyhow::Result<()> {
    let provider_messages = bb_core::agent_session::messages_to_provider(session_messages);
    let request = CompletionRequest {
        system_prompt: system_prompt.to_string(),
        messages: provider_messages,
        tools: tool_defs.to_vec(),
        extra_tool_schemas: vec![],
        model: model.to_string(),
        max_tokens,
        stream: true,
        thinking: thinking.map(ToOwned::to_owned),
    };

    hydrate_request_metrics_state_from_messages(state, &request).await
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RequestMutationFlags {
    pub system_prompt_mutated: bool,
    pub context_rewritten: bool,
    pub request_rewritten: bool,
    pub post_compaction: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RequestCacheMetrics {
    pub request_id: String,
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub turn_index: u32,
    pub context_epoch: u64,

    pub stable_prefix_hash: String,
    pub stable_prefix_bytes: usize,
    pub full_request_hash: String,
    pub provider_messages_hash: String,
    pub tool_defs_hash: String,
    pub system_prompt_hash: String,

    pub previous_request_hash: Option<String>,
    pub first_divergence_byte: Option<usize>,
    pub first_divergence_token_estimate: Option<u64>,
    pub reused_prefix_bytes_estimate: Option<usize>,
    pub reused_prefix_tokens_estimate: Option<u64>,

    pub prompt_bytes: usize,
    pub message_count: usize,
    pub tool_count: usize,

    pub cache_metrics_source: CacheMetricsSource,
    pub provider_cache_read_tokens: Option<u64>,
    pub provider_cache_write_tokens: Option<u64>,
    pub estimated_cache_read_tokens: Option<u64>,
    pub estimated_cache_write_tokens: Option<u64>,

    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub prompt_token_total: u64,
    pub cache_read_hit_rate_pct: Option<f64>,
    pub cache_effective_utilization_pct: Option<f64>,
    pub warm_request: bool,

    pub request_started_at_ms: i64,
    pub first_stream_event_at_ms: Option<i64>,
    pub first_text_delta_at_ms: Option<i64>,
    pub finished_at_ms: i64,

    pub ttft_ms: Option<u64>,
    pub total_latency_ms: u64,
    pub tool_wait_ms: u64,
    pub resume_latency_ms: Option<u64>,

    pub post_compaction: bool,
    pub system_prompt_mutated: bool,
    pub context_rewritten: bool,
    pub request_rewritten: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedRequestMetrics {
    pub request_id: String,
    pub stable_prefix_hash: String,
    pub stable_prefix_bytes: usize,
    pub full_request_hash: String,
    pub provider_messages_hash: String,
    pub tool_defs_hash: String,
    pub system_prompt_hash: String,
    pub previous_request_hash: Option<String>,
    pub first_divergence_byte: Option<usize>,
    pub first_divergence_token_estimate: Option<u64>,
    pub reused_prefix_bytes_estimate: Option<usize>,
    pub reused_prefix_tokens_estimate: Option<u64>,
    pub cacheable_prompt_bytes: usize,
    pub message_count: usize,
    pub tool_count: usize,
    pub cacheable_prompt: String,
    pub context_epoch: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedCacheUsage {
    pub cache_metrics_source: CacheMetricsSource,
    pub effective_input_tokens: u64,
    pub effective_output_tokens: u64,
    pub effective_cache_read_tokens: u64,
    pub effective_cache_write_tokens: u64,
    pub prompt_token_total: u64,
    pub provider_cache_read_tokens: Option<u64>,
    pub provider_cache_write_tokens: Option<u64>,
    pub estimated_cache_read_tokens: Option<u64>,
    pub estimated_cache_write_tokens: Option<u64>,
    pub warm_request: bool,
}

pub(crate) async fn prepare_request_metrics(
    state: &SharedRequestMetricsState,
    request: &CompletionRequest,
) -> anyhow::Result<PreparedRequestMetrics> {
    let canonical_request = canonical_json_from_serializable(request)?;
    let combined_tool_defs = combined_tool_defs(request);
    let cacheable_prompt = canonical_cacheable_prompt(
        &request.system_prompt,
        &combined_tool_defs,
        &request.messages,
    )?;
    let stable_prefix_json = canonical_json_from_value(&serde_json::json!({
        "system_prompt": request.system_prompt,
        "tools": combined_tool_defs,
    }))?;
    let provider_messages_json = canonical_json_from_value(&serde_json::json!(request.messages))?;
    let tool_defs_json = canonical_json_from_value(&serde_json::json!(combined_tool_defs))?;
    let system_prompt_json = canonical_json_from_value(&serde_json::json!(request.system_prompt))?;

    let full_request_hash = sha256_hex(canonical_request.as_bytes());
    let stable_prefix_hash = sha256_hex(stable_prefix_json.as_bytes());
    let provider_messages_hash = sha256_hex(provider_messages_json.as_bytes());
    let tool_defs_hash = sha256_hex(tool_defs_json.as_bytes());
    let system_prompt_hash = sha256_hex(system_prompt_json.as_bytes());

    let state_guard = state.lock().await;
    let previous_request_hash = state_guard.last_request_hash.clone();
    let diff = state_guard
        .last_cacheable_prompt
        .as_ref()
        .map(|previous| diff_prefix(previous, &cacheable_prompt));

    Ok(PreparedRequestMetrics {
        request_id: Uuid::new_v4().to_string(),
        stable_prefix_hash,
        stable_prefix_bytes: stable_prefix_json.len(),
        full_request_hash,
        provider_messages_hash,
        tool_defs_hash,
        system_prompt_hash,
        previous_request_hash,
        first_divergence_byte: diff.as_ref().and_then(|d| d.first_divergence_byte),
        first_divergence_token_estimate: diff.as_ref().and_then(|d| {
            d.first_divergence_byte
                .map(|bytes| estimate_tokens_from_bytes_for_model(bytes, &request.model))
        }),
        reused_prefix_bytes_estimate: diff.as_ref().map(|d| d.common_prefix_bytes),
        reused_prefix_tokens_estimate: diff
            .as_ref()
            .map(|d| estimate_tokens_from_bytes_for_model(d.common_prefix_bytes, &request.model)),
        cacheable_prompt_bytes: cacheable_prompt.len(),
        message_count: request.messages.len(),
        tool_count: request.tools.len() + request.extra_tool_schemas.len(),
        cacheable_prompt,
        context_epoch: state_guard.context_epoch,
    })
}

pub(crate) async fn commit_request_metrics_state(
    state: &SharedRequestMetricsState,
    prepared: &PreparedRequestMetrics,
) {
    let mut state_guard = state.lock().await;
    state_guard.last_request_hash = Some(prepared.full_request_hash.clone());
    state_guard.last_cacheable_prompt = Some(prepared.cacheable_prompt.clone());
}

fn combined_tool_defs(request: &CompletionRequest) -> Vec<Value> {
    request
        .tools
        .iter()
        .cloned()
        .chain(request.extra_tool_schemas.iter().cloned())
        .collect()
}

fn canonical_cacheable_prompt_from_request(request: &CompletionRequest) -> anyhow::Result<String> {
    let tool_defs = combined_tool_defs(request);
    canonical_cacheable_prompt(&request.system_prompt, &tool_defs, &request.messages)
}

fn canonical_cacheable_prompt(
    system_prompt: &str,
    tool_defs: &[Value],
    provider_messages: &[Value],
) -> anyhow::Result<String> {
    canonical_json_from_value(&serde_json::json!([
        {"tools": tool_defs},
        {"system": anthropic_system_blocks(system_prompt)},
        {"messages": anthropic_cacheable_messages(provider_messages)},
    ]))
}

fn anthropic_system_blocks(system_prompt: &str) -> Vec<Value> {
    if system_prompt.is_empty() {
        Vec::new()
    } else {
        vec![serde_json::json!({
            "type": "text",
            "text": system_prompt,
        })]
    }
}

fn anthropic_cacheable_messages(provider_messages: &[Value]) -> Vec<Value> {
    provider_messages
        .iter()
        .filter_map(anthropic_cacheable_message)
        .collect()
}

fn anthropic_cacheable_message(message: &Value) -> Option<Value> {
    let role = message.get("role")?.as_str()?;
    match role {
        "user" => Some(serde_json::json!({
            "role": "user",
            "content": anthropic_normalize_user_content(message.get("content")?),
        })),
        "assistant" => {
            let mut content = Vec::new();

            if let Some(text) = message.get("content").and_then(Value::as_str)
                && !text.is_empty()
            {
                content.push(serde_json::json!({ "type": "text", "text": text }));
            }

            if let Some(arr) = message.get("content").and_then(Value::as_array) {
                for block in arr {
                    content.push(block.clone());
                }
            }

            if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    let args_str = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let args: Value =
                        serde_json::from_str(args_str).unwrap_or_else(|_| serde_json::json!({}));
                    content.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.get("id").and_then(Value::as_str).unwrap_or(""),
                        "name": tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .cloned()
                            .unwrap_or(Value::Null),
                        "input": args,
                    }));
                }
            }

            if content.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "role": "assistant", "content": content }))
            }
        }
        "tool" => Some(serde_json::json!({
            "role": "user",
            "content": [serde_json::json!({
                "type": "tool_result",
                "tool_use_id": message.get("tool_call_id").cloned().unwrap_or(Value::Null),
                "content": anthropic_normalize_tool_content(
                    message.get("content").unwrap_or(&Value::Null),
                ),
            })],
        })),
        _ => None,
    }
}

fn anthropic_normalize_user_content(content: &Value) -> Value {
    match content {
        Value::String(text) => serde_json::json!([{ "type": "text", "text": text }]),
        Value::Array(arr) => Value::Array(arr.clone()),
        other => other.clone(),
    }
}

fn anthropic_normalize_tool_content(content: &Value) -> Value {
    match content {
        Value::Array(arr) => Value::Array(arr.clone()),
        Value::Null => Value::String(String::new()),
        other => other.clone(),
    }
}

pub(crate) fn resolve_cache_usage(
    prepared: &PreparedRequestMetrics,
    collected: &CollectedResponse,
) -> ResolvedCacheUsage {
    let provider_prompt_token_total =
        collected.input_tokens + collected.cache_read_tokens + collected.cache_write_tokens;

    match collected.cache_metrics_source {
        CacheMetricsSource::Official => ResolvedCacheUsage {
            cache_metrics_source: CacheMetricsSource::Official,
            effective_input_tokens: collected.input_tokens,
            effective_output_tokens: collected.output_tokens,
            effective_cache_read_tokens: collected.cache_read_tokens,
            effective_cache_write_tokens: collected.cache_write_tokens,
            prompt_token_total: provider_prompt_token_total,
            provider_cache_read_tokens: Some(collected.cache_read_tokens),
            provider_cache_write_tokens: Some(collected.cache_write_tokens),
            estimated_cache_read_tokens: None,
            estimated_cache_write_tokens: None,
            warm_request: collected.cache_read_tokens > 0,
        },
        CacheMetricsSource::Estimated => {
            let estimated_cache_read = prepared.reused_prefix_tokens_estimate.unwrap_or(0);
            let estimated_cache_write = 0;
            let effective_input_tokens = provider_prompt_token_total
                .saturating_sub(estimated_cache_read + estimated_cache_write);

            ResolvedCacheUsage {
                cache_metrics_source: CacheMetricsSource::Estimated,
                effective_input_tokens,
                effective_output_tokens: collected.output_tokens,
                effective_cache_read_tokens: estimated_cache_read,
                effective_cache_write_tokens: estimated_cache_write,
                prompt_token_total: provider_prompt_token_total,
                provider_cache_read_tokens: Some(collected.cache_read_tokens),
                provider_cache_write_tokens: Some(collected.cache_write_tokens),
                estimated_cache_read_tokens: Some(estimated_cache_read),
                estimated_cache_write_tokens: Some(estimated_cache_write),
                warm_request: estimated_cache_read > 0,
            }
        }
        CacheMetricsSource::Unknown => ResolvedCacheUsage {
            cache_metrics_source: CacheMetricsSource::Unknown,
            effective_input_tokens: collected.input_tokens,
            effective_output_tokens: collected.output_tokens,
            effective_cache_read_tokens: collected.cache_read_tokens,
            effective_cache_write_tokens: collected.cache_write_tokens,
            prompt_token_total: provider_prompt_token_total,
            provider_cache_read_tokens: Some(collected.cache_read_tokens),
            provider_cache_write_tokens: Some(collected.cache_write_tokens),
            estimated_cache_read_tokens: None,
            estimated_cache_write_tokens: None,
            warm_request: collected.cache_read_tokens > 0,
        },
    }
}

pub(crate) fn build_final_request_metrics(
    prepared: PreparedRequestMetrics,
    session_id: &str,
    provider: &str,
    model: &str,
    turn_index: u32,
    mutation_flags: &RequestMutationFlags,
    request_started_at_ms: i64,
    first_stream_event_at_ms: Option<i64>,
    first_text_delta_at_ms: Option<i64>,
    finished_at_ms: i64,
    usage: &ResolvedCacheUsage,
    total_latency_ms: u64,
    tool_wait_ms: u64,
    resume_latency_ms: Option<u64>,
) -> RequestCacheMetrics {
    let cache_read_hit_rate_pct = cache_read_hit_rate_pct(
        usage.effective_input_tokens,
        usage.effective_cache_read_tokens,
    );
    let cache_effective_utilization_pct = cache_effective_utilization_pct(
        usage.effective_input_tokens,
        usage.effective_cache_read_tokens,
        usage.effective_cache_write_tokens,
    );

    RequestCacheMetrics {
        request_id: prepared.request_id,
        session_id: session_id.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        turn_index,
        context_epoch: prepared.context_epoch,

        stable_prefix_hash: prepared.stable_prefix_hash,
        stable_prefix_bytes: prepared.stable_prefix_bytes,
        full_request_hash: prepared.full_request_hash,
        provider_messages_hash: prepared.provider_messages_hash,
        tool_defs_hash: prepared.tool_defs_hash,
        system_prompt_hash: prepared.system_prompt_hash,

        previous_request_hash: prepared.previous_request_hash,
        first_divergence_byte: prepared.first_divergence_byte,
        first_divergence_token_estimate: prepared.first_divergence_token_estimate,
        reused_prefix_bytes_estimate: prepared.reused_prefix_bytes_estimate,
        reused_prefix_tokens_estimate: prepared.reused_prefix_tokens_estimate,

        prompt_bytes: prepared.cacheable_prompt_bytes,
        message_count: prepared.message_count,
        tool_count: prepared.tool_count,

        cache_metrics_source: usage.cache_metrics_source.clone(),
        provider_cache_read_tokens: usage.provider_cache_read_tokens,
        provider_cache_write_tokens: usage.provider_cache_write_tokens,
        estimated_cache_read_tokens: usage.estimated_cache_read_tokens,
        estimated_cache_write_tokens: usage.estimated_cache_write_tokens,

        cache_read_tokens: usage.effective_cache_read_tokens,
        cache_write_tokens: usage.effective_cache_write_tokens,
        input_tokens: usage.effective_input_tokens,
        output_tokens: usage.effective_output_tokens,
        prompt_token_total: usage.prompt_token_total,
        cache_read_hit_rate_pct,
        cache_effective_utilization_pct,
        warm_request: usage.warm_request,

        request_started_at_ms,
        first_stream_event_at_ms,
        first_text_delta_at_ms,
        finished_at_ms,

        ttft_ms: first_text_delta_at_ms
            .map(|first| first.saturating_sub(request_started_at_ms) as u64),
        total_latency_ms,
        tool_wait_ms,
        resume_latency_ms,

        post_compaction: mutation_flags.post_compaction,
        system_prompt_mutated: mutation_flags.system_prompt_mutated,
        context_rewritten: mutation_flags.context_rewritten,
        request_rewritten: mutation_flags.request_rewritten,
    }
}

pub(crate) fn append_request_metrics_log(metrics: &RequestCacheMetrics) -> anyhow::Result<()> {
    let _guard = REQUEST_METRICS_FILE_LOCK
        .lock()
        .expect("request metrics file lock");
    let dir = config::global_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("request-metrics.jsonl");
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, metrics)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

pub(crate) fn cache_read_hit_rate_pct(input_tokens: u64, cache_read_tokens: u64) -> Option<f64> {
    let total = input_tokens + cache_read_tokens;
    if total == 0 {
        None
    } else {
        Some(cache_read_tokens as f64 * 100.0 / total as f64)
    }
}

pub(crate) fn cache_effective_utilization_pct(
    input_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
) -> Option<f64> {
    let total = input_tokens + cache_read_tokens + cache_write_tokens;
    if total == 0 {
        None
    } else {
        Some(cache_read_tokens as f64 * 100.0 / total as f64)
    }
}

fn canonical_json_from_serializable<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let json = serde_json::to_value(value)?;
    canonical_json_from_value(&json)
}

fn canonical_json_from_value(value: &Value) -> anyhow::Result<String> {
    let mut out = String::new();
    write_canonical_json(value, &mut out)?;
    Ok(out)
}

fn write_canonical_json(value: &Value, out: &mut String) -> anyhow::Result<()> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&serde_json::to_string(s)?),
        Value::Array(arr) => {
            out.push('[');
            for (idx, item) in arr.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                write_canonical_json(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
            keys.sort_unstable();
            for (idx, key) in keys.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key)?);
                out.push(':');
                write_canonical_json(&map[*key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PrefixDiff {
    first_divergence_byte: Option<usize>,
    common_prefix_bytes: usize,
}

fn diff_prefix(previous: &str, current: &str) -> PrefixDiff {
    let previous_bytes = previous.as_bytes();
    let current_bytes = current.as_bytes();
    let common_prefix_bytes = previous_bytes
        .iter()
        .zip(current_bytes.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let first_divergence_byte = if previous_bytes.len() == current_bytes.len()
        && common_prefix_bytes == previous_bytes.len()
    {
        None
    } else {
        Some(common_prefix_bytes)
    };

    PrefixDiff {
        first_divergence_byte,
        common_prefix_bytes,
    }
}

fn estimate_tokens_from_bytes_for_model(bytes: usize, model: &str) -> u64 {
    let bytes_per_token = if model.contains("claude") { 3.45 } else { 4.0 };
    ((bytes as f64) / bytes_per_token).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::types::{AgentMessage, ContentBlock, UserMessage};
    use serde_json::json;

    #[test]
    fn cache_hit_rate_uses_cached_read_over_total_prompt_tokens() {
        let pct = cache_read_hit_rate_pct(100, 300).expect("pct");
        assert!((pct - 75.0).abs() < 1e-9);
    }

    #[test]
    fn cache_effective_utilization_accounts_for_cache_writes() {
        let pct = cache_effective_utilization_pct(100, 300, 100).expect("pct");
        assert!((pct - 60.0).abs() < 1e-9);
    }

    #[test]
    fn cache_hit_rate_returns_none_for_zero_total() {
        assert_eq!(cache_read_hit_rate_pct(0, 0), None);
        assert_eq!(cache_effective_utilization_pct(0, 0, 0), None);
    }

    #[test]
    fn canonical_json_sorts_object_keys() {
        let value = json!({"b": 2, "a": 1, "c": {"y": 2, "x": 1}});
        let canonical = canonical_json_from_value(&value).expect("canonical");
        assert_eq!(canonical, r#"{"a":1,"b":2,"c":{"x":1,"y":2}}"#);
    }

    #[test]
    fn diff_prefix_reports_first_divergence_and_common_prefix() {
        let diff = diff_prefix("abcdef", "abcXYZ");
        assert_eq!(diff.common_prefix_bytes, 3);
        assert_eq!(diff.first_divergence_byte, Some(3));
    }

    #[test]
    fn canonical_cacheable_prompt_tracks_anthropic_message_shape() {
        let request = CompletionRequest {
            system_prompt: "system".to_string(),
            messages: vec![
                serde_json::json!({"role": "user", "content": "hello"}),
                serde_json::json!({
                    "role": "assistant",
                    "content": "ok",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "demo",
                            "arguments": "{\"x\":1}"
                        }
                    }]
                }),
                serde_json::json!({
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "done"
                }),
            ],
            tools: vec![serde_json::json!({
                "type": "function",
                "function": {
                    "name": "demo",
                    "description": "desc",
                    "parameters": {"type": "object"}
                }
            })],
            extra_tool_schemas: vec![],
            model: "claude-sonnet-4-6".to_string(),
            max_tokens: Some(42),
            stream: true,
            thinking: None,
        };

        let canonical = canonical_cacheable_prompt_from_request(&request).expect("canonical");
        assert!(canonical.contains("\"system\":[{\"text\":\"system\",\"type\":\"text\"}]"));
        assert!(canonical.contains("\"tool_use_id\":\"call_1\""));
        assert!(canonical.contains("\"type\":\"tool_use\""));
        assert!(canonical.contains("\"content\":[{\"text\":\"hello\",\"type\":\"text\"}]"));
    }

    #[test]
    fn resolve_cache_usage_prefers_provider_values_for_official_metrics() {
        let prepared = PreparedRequestMetrics {
            request_id: "req".to_string(),
            stable_prefix_hash: "stable".to_string(),
            stable_prefix_bytes: 10,
            full_request_hash: "full".to_string(),
            provider_messages_hash: "messages".to_string(),
            tool_defs_hash: "tools".to_string(),
            system_prompt_hash: "system".to_string(),
            previous_request_hash: Some("prev".to_string()),
            first_divergence_byte: Some(10),
            first_divergence_token_estimate: Some(3),
            reused_prefix_bytes_estimate: Some(40),
            reused_prefix_tokens_estimate: Some(10),
            cacheable_prompt_bytes: 80,
            message_count: 1,
            tool_count: 0,
            cacheable_prompt: "prompt".to_string(),
            context_epoch: 0,
        };
        let collected = CollectedResponse {
            text: String::new(),
            thinking: String::new(),
            tool_calls: vec![],
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 40,
            cache_write_tokens: 5,
            cache_metrics_source: CacheMetricsSource::Official,
        };

        let resolved = resolve_cache_usage(&prepared, &collected);
        assert_eq!(resolved.cache_metrics_source, CacheMetricsSource::Official);
        assert_eq!(resolved.effective_input_tokens, 100);
        assert_eq!(resolved.effective_cache_read_tokens, 40);
        assert_eq!(resolved.effective_cache_write_tokens, 5);
        assert_eq!(resolved.provider_cache_read_tokens, Some(40));
        assert_eq!(resolved.estimated_cache_read_tokens, None);
    }

    #[test]
    fn resolve_cache_usage_uses_prefix_estimate_for_estimated_metrics() {
        let prepared = PreparedRequestMetrics {
            request_id: "req".to_string(),
            stable_prefix_hash: "stable".to_string(),
            stable_prefix_bytes: 10,
            full_request_hash: "full".to_string(),
            provider_messages_hash: "messages".to_string(),
            tool_defs_hash: "tools".to_string(),
            system_prompt_hash: "system".to_string(),
            previous_request_hash: Some("prev".to_string()),
            first_divergence_byte: Some(10),
            first_divergence_token_estimate: Some(3),
            reused_prefix_bytes_estimate: Some(48),
            reused_prefix_tokens_estimate: Some(12),
            cacheable_prompt_bytes: 96,
            message_count: 1,
            tool_count: 0,
            cacheable_prompt: "prompt".to_string(),
            context_epoch: 0,
        };
        let collected = CollectedResponse {
            text: String::new(),
            thinking: String::new(),
            tool_calls: vec![],
            input_tokens: 70,
            output_tokens: 15,
            cache_read_tokens: 20,
            cache_write_tokens: 3,
            cache_metrics_source: CacheMetricsSource::Estimated,
        };

        let resolved = resolve_cache_usage(&prepared, &collected);
        assert_eq!(resolved.cache_metrics_source, CacheMetricsSource::Estimated);
        assert_eq!(resolved.prompt_token_total, 93);
        assert_eq!(resolved.effective_cache_read_tokens, 12);
        assert_eq!(resolved.effective_cache_write_tokens, 0);
        assert_eq!(resolved.effective_input_tokens, 81);
        assert_eq!(resolved.provider_cache_read_tokens, Some(20));
        assert_eq!(resolved.estimated_cache_read_tokens, Some(12));
        assert!(resolved.warm_request);
    }

    #[tokio::test]
    async fn hydrate_state_from_session_messages_seeds_previous_request_hash() {
        let state = new_shared_request_metrics_state();
        let session_messages = vec![AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            timestamp: 0,
        })];

        hydrate_request_metrics_state_from_session_messages(
            &state,
            "system",
            &[],
            &session_messages,
            "dummy-model",
            Some(42),
            None,
        )
        .await
        .expect("hydrate state");

        let request = CompletionRequest {
            system_prompt: "system".to_string(),
            messages: bb_core::agent_session::messages_to_provider(&session_messages),
            tools: vec![],
            extra_tool_schemas: vec![],
            model: "dummy-model".to_string(),
            max_tokens: Some(42),
            stream: true,
            thinking: None,
        };

        let prepared = prepare_request_metrics(&state, &request)
            .await
            .expect("prepare metrics");

        assert!(prepared.previous_request_hash.is_some());
        assert_eq!(prepared.first_divergence_byte, None);
        assert_eq!(
            prepared.reused_prefix_bytes_estimate,
            Some(prepared.cacheable_prompt_bytes)
        );
        assert!(prepared.reused_prefix_bytes_estimate.unwrap_or_default() > 0);
    }
}
