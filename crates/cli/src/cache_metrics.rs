use bb_core::config;
use bb_core::types::AgentMessage;
use bb_provider::CompletionRequest;
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
    pub last_canonical_request: Option<String>,
    pub context_epoch: u64,
}

pub(crate) type SharedRequestMetricsState = Arc<Mutex<RequestMetricsState>>;

pub(crate) fn new_shared_request_metrics_state() -> SharedRequestMetricsState {
    Arc::new(Mutex::new(RequestMetricsState::default()))
}

pub(crate) async fn hydrate_request_metrics_state_from_messages(
    state: &SharedRequestMetricsState,
    system_prompt: &str,
    tool_defs: &[Value],
    provider_messages: &[Value],
) -> anyhow::Result<()> {
    let request = CompletionRequest {
        system_prompt: system_prompt.to_string(),
        messages: provider_messages.to_vec(),
        tools: tool_defs.to_vec(),
        extra_tool_schemas: vec![],
        model: String::new(),
        max_tokens: None,
        stream: true,
        thinking: None,
    };

    let canonical_request = canonical_json_from_serializable(&request)?;
    let full_request_hash = sha256_hex(canonical_request.as_bytes());

    let mut state_guard = state.lock().await;
    state_guard.last_request_hash = Some(full_request_hash);
    state_guard.last_canonical_request = Some(canonical_request);
    Ok(())
}

pub(crate) async fn hydrate_request_metrics_state_from_session_messages(
    state: &SharedRequestMetricsState,
    system_prompt: &str,
    tool_defs: &[Value],
    session_messages: &[AgentMessage],
) -> anyhow::Result<()> {
    let provider_messages = bb_core::agent_session::messages_to_provider(session_messages);
    hydrate_request_metrics_state_from_messages(state, system_prompt, tool_defs, &provider_messages)
        .await
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
    pub prompt_bytes: usize,
    pub message_count: usize,
    pub tool_count: usize,
    pub canonical_request: String,
    pub context_epoch: u64,
}

pub(crate) async fn prepare_request_metrics(
    state: &SharedRequestMetricsState,
    request: &CompletionRequest,
) -> anyhow::Result<PreparedRequestMetrics> {
    let canonical_request = canonical_json_from_serializable(request)?;
    let stable_prefix_json = canonical_json_from_value(&serde_json::json!({
        "system_prompt": request.system_prompt,
        "tools": request.tools,
    }))?;
    let provider_messages_json = canonical_json_from_value(&serde_json::json!(request.messages))?;
    let tool_defs_json = canonical_json_from_value(&serde_json::json!(request.tools))?;
    let system_prompt_json = canonical_json_from_value(&serde_json::json!(request.system_prompt))?;

    let full_request_hash = sha256_hex(canonical_request.as_bytes());
    let stable_prefix_hash = sha256_hex(stable_prefix_json.as_bytes());
    let provider_messages_hash = sha256_hex(provider_messages_json.as_bytes());
    let tool_defs_hash = sha256_hex(tool_defs_json.as_bytes());
    let system_prompt_hash = sha256_hex(system_prompt_json.as_bytes());

    let state_guard = state.lock().await;
    let previous_request_hash = state_guard.last_request_hash.clone();
    let diff = state_guard
        .last_canonical_request
        .as_ref()
        .map(|previous| diff_prefix(previous, &canonical_request));

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
        first_divergence_token_estimate: diff
            .as_ref()
            .and_then(|d| d.first_divergence_byte.map(estimate_tokens_from_bytes)),
        reused_prefix_bytes_estimate: diff.as_ref().map(|d| d.common_prefix_bytes),
        reused_prefix_tokens_estimate: diff
            .as_ref()
            .map(|d| estimate_tokens_from_bytes(d.common_prefix_bytes)),
        prompt_bytes: canonical_request.len(),
        message_count: request.messages.len(),
        tool_count: request.tools.len(),
        canonical_request,
        context_epoch: state_guard.context_epoch,
    })
}

pub(crate) async fn commit_request_metrics_state(
    state: &SharedRequestMetricsState,
    prepared: &PreparedRequestMetrics,
) {
    let mut state_guard = state.lock().await;
    state_guard.last_request_hash = Some(prepared.full_request_hash.clone());
    state_guard.last_canonical_request = Some(prepared.canonical_request.clone());
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
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    total_latency_ms: u64,
    tool_wait_ms: u64,
    resume_latency_ms: Option<u64>,
) -> RequestCacheMetrics {
    let prompt_token_total = input_tokens + cache_read_tokens + cache_write_tokens;
    let cache_read_hit_rate_pct = cache_read_hit_rate_pct(input_tokens, cache_read_tokens);
    let cache_effective_utilization_pct =
        cache_effective_utilization_pct(input_tokens, cache_read_tokens, cache_write_tokens);

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

        prompt_bytes: prepared.prompt_bytes,
        message_count: prepared.message_count,
        tool_count: prepared.tool_count,

        cache_read_tokens,
        cache_write_tokens,
        input_tokens,
        output_tokens,
        prompt_token_total,
        cache_read_hit_rate_pct,
        cache_effective_utilization_pct,
        warm_request: cache_read_tokens > 0,

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

fn estimate_tokens_from_bytes(bytes: usize) -> u64 {
    (bytes as u64).div_ceil(4)
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
        assert!(prepared.first_divergence_byte.is_some());
        assert_eq!(
            prepared.reused_prefix_bytes_estimate,
            prepared.first_divergence_byte
        );
        assert!(prepared.reused_prefix_bytes_estimate.unwrap_or_default() > 0);
    }
}
