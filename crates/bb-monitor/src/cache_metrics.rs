use serde::{Deserialize, Serialize};

/// Provider-reported cache attribution quality.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheMetricsSource {
    /// The provider did not expose enough cache information to classify the request.
    #[default]
    Unknown,
    /// Cache read/write numbers came directly from provider usage events.
    Official,
    /// Cache read/write numbers were estimated from prompt-prefix reuse heuristics.
    Estimated,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestMutationFlags {
    pub system_prompt_mutated: bool,
    pub context_rewritten: bool,
    pub request_rewritten: bool,
    pub post_compaction: bool,
}

/// Final Phase-1 request-level cache/latency record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RequestCacheMetrics {
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

/// Prepared request state captured before the provider call starts.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedRequestMetrics {
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

/// Resolved usage numbers after combining provider-reported and estimated cache signals.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedCacheUsage {
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

pub fn cache_read_hit_rate_pct(input_tokens: u64, cache_read_tokens: u64) -> Option<f64> {
    let total = input_tokens + cache_read_tokens;
    if total == 0 {
        None
    } else {
        Some(cache_read_tokens as f64 * 100.0 / total as f64)
    }
}

pub fn cache_effective_utilization_pct(
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

#[cfg(test)]
mod tests {
    use super::{cache_effective_utilization_pct, cache_read_hit_rate_pct};

    #[test]
    fn cache_hit_rate_is_empty_without_prompt_tokens() {
        assert_eq!(cache_read_hit_rate_pct(0, 0), None);
    }

    #[test]
    fn cache_hit_rate_uses_input_plus_cache_read_total() {
        let pct = cache_read_hit_rate_pct(80, 20).expect("percentage");
        assert!((pct - 20.0).abs() < 1e-9);
    }

    #[test]
    fn cache_effective_utilization_includes_cache_write() {
        let pct = cache_effective_utilization_pct(70, 20, 10).expect("percentage");
        assert!((pct - 20.0).abs() < 1e-9);
    }
}
