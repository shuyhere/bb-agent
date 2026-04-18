use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::cache_metrics::{
    CacheMetricsSource, cache_effective_utilization_pct, cache_read_hit_rate_pct,
};

use super::canonical::{canonical_cacheable_prompt, canonical_json_from_serializable};
use super::divergence::{diff_prefix, estimate_tokens_from_bytes_for_model};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestMetricsState {
    pub last_request_hash: Option<String>,
    pub last_cacheable_prompt: Option<String>,
    pub context_epoch: u64,
}

#[derive(Clone, Debug, Default)]
pub struct RequestMetricsTracker {
    state: RequestMetricsState,
}

impl RequestMetricsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_state(state: RequestMetricsState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &RequestMetricsState {
        &self.state
    }

    pub fn into_state(self) -> RequestMetricsState {
        self.state
    }

    pub fn increment_context_epoch(&mut self) {
        self.state.context_epoch = self.state.context_epoch.saturating_add(1);
    }

    pub fn hydrate(&mut self, snapshot: &RequestMetricsSnapshot) -> Result<()> {
        hydrate_request_metrics_state(&mut self.state, snapshot)
    }

    pub fn prepare(&self, snapshot: &RequestMetricsSnapshot) -> Result<PreparedRequestMetrics> {
        prepare_request_metrics(&self.state, snapshot)
    }

    pub fn commit(&mut self, prepared: &PreparedRequestMetrics) {
        commit_request_metrics_state(&mut self.state, prepared);
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RequestMetricsSnapshot {
    pub system_prompt: String,
    pub provider_messages: Vec<Value>,
    pub tool_definitions: Vec<Value>,
    pub extra_tool_definitions: Vec<Value>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub thinking: Option<String>,
}

impl RequestMetricsSnapshot {
    pub fn combined_tool_definitions(&self) -> Vec<Value> {
        self.tool_definitions
            .iter()
            .cloned()
            .chain(self.extra_tool_definitions.iter().cloned())
            .collect()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestMutationFlags {
    pub system_prompt_mutated: bool,
    pub context_rewritten: bool,
    pub request_rewritten: bool,
    pub post_compaction: bool,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResponseUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_metrics_source: CacheMetricsSource,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestMetricsIdentity {
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub turn_index: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestMetricsTiming {
    pub request_started_at_ms: i64,
    pub first_stream_event_at_ms: Option<i64>,
    pub first_text_delta_at_ms: Option<i64>,
    pub finished_at_ms: i64,
    pub total_latency_ms: u64,
    pub tool_wait_ms: u64,
    pub resume_latency_ms: Option<u64>,
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

pub fn hydrate_request_metrics_state(
    state: &mut RequestMetricsState,
    snapshot: &RequestMetricsSnapshot,
) -> Result<()> {
    let canonical_request = canonical_json_from_serializable(snapshot)?;
    let full_request_hash = sha256_hex(canonical_request.as_bytes());
    let cacheable_prompt = canonical_cacheable_prompt(snapshot)?;

    state.last_request_hash = Some(full_request_hash);
    state.last_cacheable_prompt = Some(cacheable_prompt);
    Ok(())
}

pub fn prepare_request_metrics(
    state: &RequestMetricsState,
    snapshot: &RequestMetricsSnapshot,
) -> Result<PreparedRequestMetrics> {
    let canonical_request = canonical_json_from_serializable(snapshot)?;
    let combined_tool_defs = snapshot.combined_tool_definitions();
    let cacheable_prompt = canonical_cacheable_prompt(snapshot)?;
    let stable_prefix_json = canonical_json_from_serializable(&serde_json::json!({
        "system_prompt": snapshot.system_prompt,
        "tools": combined_tool_defs,
    }))?;
    let provider_messages_json = canonical_json_from_serializable(&snapshot.provider_messages)?;
    let tool_defs_json = canonical_json_from_serializable(&combined_tool_defs)?;
    let system_prompt_json = canonical_json_from_serializable(&snapshot.system_prompt)?;

    let full_request_hash = sha256_hex(canonical_request.as_bytes());
    let stable_prefix_hash = sha256_hex(stable_prefix_json.as_bytes());
    let provider_messages_hash = sha256_hex(provider_messages_json.as_bytes());
    let tool_defs_hash = sha256_hex(tool_defs_json.as_bytes());
    let system_prompt_hash = sha256_hex(system_prompt_json.as_bytes());

    let previous_request_hash = state.last_request_hash.clone();
    let diff = state
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
                .map(|bytes| estimate_tokens_from_bytes_for_model(bytes, &snapshot.model))
        }),
        reused_prefix_bytes_estimate: diff.as_ref().map(|d| d.common_prefix_bytes),
        reused_prefix_tokens_estimate: diff
            .as_ref()
            .map(|d| estimate_tokens_from_bytes_for_model(d.common_prefix_bytes, &snapshot.model)),
        cacheable_prompt_bytes: cacheable_prompt.len(),
        message_count: snapshot.provider_messages.len(),
        tool_count: snapshot.tool_definitions.len() + snapshot.extra_tool_definitions.len(),
        cacheable_prompt,
        context_epoch: state.context_epoch,
    })
}

pub fn commit_request_metrics_state(
    state: &mut RequestMetricsState,
    prepared: &PreparedRequestMetrics,
) {
    state.last_request_hash = Some(prepared.full_request_hash.clone());
    state.last_cacheable_prompt = Some(prepared.cacheable_prompt.clone());
}

pub fn resolve_cache_usage(
    prepared: &PreparedRequestMetrics,
    usage: &ResponseUsage,
) -> ResolvedCacheUsage {
    let provider_prompt_token_total =
        usage.input_tokens + usage.cache_read_tokens + usage.cache_write_tokens;

    match usage.cache_metrics_source {
        CacheMetricsSource::Official => ResolvedCacheUsage {
            cache_metrics_source: CacheMetricsSource::Official,
            effective_input_tokens: usage.input_tokens,
            effective_output_tokens: usage.output_tokens,
            effective_cache_read_tokens: usage.cache_read_tokens,
            effective_cache_write_tokens: usage.cache_write_tokens,
            prompt_token_total: provider_prompt_token_total,
            provider_cache_read_tokens: Some(usage.cache_read_tokens),
            provider_cache_write_tokens: Some(usage.cache_write_tokens),
            estimated_cache_read_tokens: None,
            estimated_cache_write_tokens: None,
            warm_request: usage.cache_read_tokens > 0,
        },
        CacheMetricsSource::Estimated => {
            let estimated_cache_read =
                normalized_estimated_cache_read_tokens(prepared, provider_prompt_token_total);
            let estimated_cache_write = 0;
            let effective_input_tokens = provider_prompt_token_total
                .saturating_sub(estimated_cache_read + estimated_cache_write);

            ResolvedCacheUsage {
                cache_metrics_source: CacheMetricsSource::Estimated,
                effective_input_tokens,
                effective_output_tokens: usage.output_tokens,
                effective_cache_read_tokens: estimated_cache_read,
                effective_cache_write_tokens: estimated_cache_write,
                prompt_token_total: provider_prompt_token_total,
                provider_cache_read_tokens: Some(usage.cache_read_tokens),
                provider_cache_write_tokens: Some(usage.cache_write_tokens),
                estimated_cache_read_tokens: Some(estimated_cache_read),
                estimated_cache_write_tokens: Some(estimated_cache_write),
                warm_request: estimated_cache_read > 0,
            }
        }
        CacheMetricsSource::Unknown => ResolvedCacheUsage {
            cache_metrics_source: CacheMetricsSource::Unknown,
            effective_input_tokens: usage.input_tokens,
            effective_output_tokens: usage.output_tokens,
            effective_cache_read_tokens: usage.cache_read_tokens,
            effective_cache_write_tokens: usage.cache_write_tokens,
            prompt_token_total: provider_prompt_token_total,
            provider_cache_read_tokens: Some(usage.cache_read_tokens),
            provider_cache_write_tokens: Some(usage.cache_write_tokens),
            estimated_cache_read_tokens: None,
            estimated_cache_write_tokens: None,
            warm_request: usage.cache_read_tokens > 0,
        },
    }
}

fn normalized_estimated_cache_read_tokens(
    prepared: &PreparedRequestMetrics,
    prompt_token_total: u64,
) -> u64 {
    if prompt_token_total == 0 {
        return 0;
    }

    let reused_prefix_bytes = prepared.reused_prefix_bytes_estimate.unwrap_or(0);
    let cacheable_prompt_bytes = prepared.cacheable_prompt_bytes;
    if reused_prefix_bytes == 0 || cacheable_prompt_bytes == 0 {
        return 0;
    }

    let reuse_ratio = reused_prefix_bytes as f64 / cacheable_prompt_bytes as f64;
    let mut estimated = (prompt_token_total as f64 * reuse_ratio).round() as u64;
    estimated = estimated.min(prompt_token_total);

    if reused_prefix_bytes < cacheable_prompt_bytes && estimated >= prompt_token_total {
        prompt_token_total.saturating_sub(1)
    } else {
        estimated
    }
}

pub fn build_final_request_metrics(
    prepared: PreparedRequestMetrics,
    identity: &RequestMetricsIdentity,
    mutation_flags: &RequestMutationFlags,
    timing: &RequestMetricsTiming,
    usage: &ResolvedCacheUsage,
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
        session_id: identity.session_id.clone(),
        provider: identity.provider.clone(),
        model: identity.model.clone(),
        turn_index: identity.turn_index,
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

        request_started_at_ms: timing.request_started_at_ms,
        first_stream_event_at_ms: timing.first_stream_event_at_ms,
        first_text_delta_at_ms: timing.first_text_delta_at_ms,
        finished_at_ms: timing.finished_at_ms,

        ttft_ms: timing
            .first_text_delta_at_ms
            .map(|first| first.saturating_sub(timing.request_started_at_ms) as u64),
        total_latency_ms: timing.total_latency_ms,
        tool_wait_ms: timing.tool_wait_ms,
        resume_latency_ms: timing.resume_latency_ms,

        post_compaction: mutation_flags.post_compaction,
        system_prompt_mutated: mutation_flags.system_prompt_mutated,
        context_rewritten: mutation_flags.context_rewritten,
        request_rewritten: mutation_flags.request_rewritten,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{
        CacheMetricsSource, PreparedRequestMetrics, RequestCacheMetrics, RequestMetricsIdentity,
        RequestMetricsSnapshot, RequestMetricsState, RequestMetricsTiming, RequestMetricsTracker,
        RequestMutationFlags, ResponseUsage, build_final_request_metrics,
        hydrate_request_metrics_state, normalized_estimated_cache_read_tokens,
        prepare_request_metrics, resolve_cache_usage,
    };
    use bb_core::types::{AgentMessage, ContentBlock, UserMessage};
    use serde_json::json;

    fn parity_test_snapshot(
        provider_messages: Vec<serde_json::Value>,
        tool_definitions: Vec<serde_json::Value>,
    ) -> RequestMetricsSnapshot {
        RequestMetricsSnapshot {
            system_prompt: "system prompt with stable cacheable instructions".to_string(),
            provider_messages,
            tool_definitions,
            extra_tool_definitions: vec![],
            model: "claude-sonnet-4-6".to_string(),
            max_tokens: Some(512),
            stream: true,
            thinking: None,
        }
    }

    fn response_usage_with_source(
        prompt_token_total: u64,
        cache_read_tokens: u64,
        output_tokens: u64,
        cache_metrics_source: CacheMetricsSource,
    ) -> ResponseUsage {
        ResponseUsage {
            input_tokens: prompt_token_total.saturating_sub(cache_read_tokens),
            output_tokens,
            cache_read_tokens,
            cache_write_tokens: 0,
            cache_metrics_source,
        }
    }

    fn assert_estimate_close_to_official(
        estimated: &RequestCacheMetrics,
        official: &RequestCacheMetrics,
        max_token_delta: u64,
        max_rate_delta_pct: f64,
    ) {
        assert_eq!(estimated.prompt_token_total, official.prompt_token_total);
        assert_eq!(estimated.output_tokens, official.output_tokens);
        assert_eq!(estimated.cache_write_tokens, 0);
        assert_eq!(official.cache_write_tokens, 0);

        let token_delta = estimated
            .cache_read_tokens
            .abs_diff(official.cache_read_tokens);
        assert!(
            token_delta <= max_token_delta,
            "cache read token delta {token_delta} exceeded tolerance {max_token_delta} (estimated={}, official={})",
            estimated.cache_read_tokens,
            official.cache_read_tokens,
        );

        let estimated_hit_rate = estimated
            .cache_read_hit_rate_pct
            .expect("estimated hit rate");
        let official_hit_rate = official.cache_read_hit_rate_pct.expect("official hit rate");
        let hit_rate_delta = (estimated_hit_rate - official_hit_rate).abs();
        assert!(
            hit_rate_delta <= max_rate_delta_pct,
            "cache hit rate delta {hit_rate_delta:.3} exceeded tolerance {max_rate_delta_pct:.3} (estimated={estimated_hit_rate:.3}, official={official_hit_rate:.3})",
        );

        let estimated_util = estimated
            .cache_effective_utilization_pct
            .expect("estimated utilization");
        let official_util = official
            .cache_effective_utilization_pct
            .expect("official utilization");
        let util_delta = (estimated_util - official_util).abs();
        assert!(
            util_delta <= max_rate_delta_pct,
            "cache utilization delta {util_delta:.3} exceeded tolerance {max_rate_delta_pct:.3} (estimated={estimated_util:.3}, official={official_util:.3})",
        );
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
        let usage = ResponseUsage {
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: 40,
            cache_write_tokens: 5,
            cache_metrics_source: CacheMetricsSource::Official,
        };

        let resolved = resolve_cache_usage(&prepared, &usage);
        assert_eq!(resolved.cache_metrics_source, CacheMetricsSource::Official);
        assert_eq!(resolved.effective_input_tokens, 100);
        assert_eq!(resolved.effective_cache_read_tokens, 40);
        assert_eq!(resolved.effective_cache_write_tokens, 5);
        assert_eq!(resolved.provider_cache_read_tokens, Some(40));
        assert_eq!(resolved.estimated_cache_read_tokens, None);
    }

    #[test]
    fn resolve_cache_usage_uses_normalized_prefix_estimate_for_estimated_metrics() {
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
        let usage = ResponseUsage {
            input_tokens: 70,
            output_tokens: 15,
            cache_read_tokens: 20,
            cache_write_tokens: 3,
            cache_metrics_source: CacheMetricsSource::Estimated,
        };

        let resolved = resolve_cache_usage(&prepared, &usage);
        assert_eq!(resolved.cache_metrics_source, CacheMetricsSource::Estimated);
        assert_eq!(resolved.prompt_token_total, 93);
        assert_eq!(resolved.effective_cache_read_tokens, 47);
        assert_eq!(resolved.effective_cache_write_tokens, 0);
        assert_eq!(resolved.effective_input_tokens, 46);
        assert_eq!(resolved.provider_cache_read_tokens, Some(20));
        assert_eq!(resolved.estimated_cache_read_tokens, Some(47));
        assert!(resolved.warm_request);
    }

    #[test]
    fn normalized_estimate_does_not_peg_changed_prompts_to_hundred_percent() {
        let prepared = PreparedRequestMetrics {
            request_id: "req".to_string(),
            stable_prefix_hash: "stable".to_string(),
            stable_prefix_bytes: 10,
            full_request_hash: "full".to_string(),
            provider_messages_hash: "messages".to_string(),
            tool_defs_hash: "tools".to_string(),
            system_prompt_hash: "system".to_string(),
            previous_request_hash: Some("prev".to_string()),
            first_divergence_byte: Some(990),
            first_divergence_token_estimate: Some(248),
            reused_prefix_bytes_estimate: Some(999),
            reused_prefix_tokens_estimate: Some(1_100),
            cacheable_prompt_bytes: 1_000,
            message_count: 1,
            tool_count: 0,
            cacheable_prompt: "prompt".to_string(),
            context_epoch: 0,
        };

        let estimated = normalized_estimated_cache_read_tokens(&prepared, 1_000);
        assert_eq!(estimated, 999);

        let resolved = resolve_cache_usage(
            &prepared,
            &ResponseUsage {
                input_tokens: 1_000,
                output_tokens: 10,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                cache_metrics_source: CacheMetricsSource::Estimated,
            },
        );
        assert_eq!(resolved.effective_cache_read_tokens, 999);
        assert_eq!(resolved.effective_input_tokens, 1);
    }

    #[test]
    fn build_final_request_metrics_computes_latency_and_rates() {
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
            context_epoch: 2,
        };
        let resolved = resolve_cache_usage(
            &prepared,
            &ResponseUsage {
                input_tokens: 70,
                output_tokens: 15,
                cache_read_tokens: 20,
                cache_write_tokens: 3,
                cache_metrics_source: CacheMetricsSource::Estimated,
            },
        );
        let metrics = build_final_request_metrics(
            prepared,
            &RequestMetricsIdentity {
                session_id: "session-1".to_string(),
                provider: "anthropic".to_string(),
                model: "claude-sonnet".to_string(),
                turn_index: 4,
            },
            &RequestMutationFlags {
                request_rewritten: true,
                ..Default::default()
            },
            &RequestMetricsTiming {
                request_started_at_ms: 100,
                first_stream_event_at_ms: Some(110),
                first_text_delta_at_ms: Some(130),
                finished_at_ms: 200,
                total_latency_ms: 100,
                tool_wait_ms: 7,
                resume_latency_ms: Some(5),
            },
            &resolved,
        );

        assert_eq!(metrics.context_epoch, 2);
        assert_eq!(metrics.turn_index, 4);
        assert_eq!(metrics.ttft_ms, Some(30));
        assert_eq!(metrics.total_latency_ms, 100);
        assert_eq!(metrics.tool_wait_ms, 7);
        assert_eq!(metrics.resume_latency_ms, Some(5));
        assert_eq!(metrics.request_rewritten, true);
        assert!(metrics.cache_read_hit_rate_pct.is_some());
        assert!(metrics.cache_effective_utilization_pct.is_some());
    }

    #[test]
    fn hydrate_state_seeds_previous_request_hash() {
        let session_messages = vec![AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
            }],
            timestamp: 0,
        })];
        let provider_messages = bb_core::agent_session::messages_to_provider(&session_messages);
        let snapshot = RequestMetricsSnapshot {
            system_prompt: "system".to_string(),
            provider_messages,
            tool_definitions: vec![],
            extra_tool_definitions: vec![],
            model: "dummy-model".to_string(),
            max_tokens: Some(42),
            stream: true,
            thinking: None,
        };

        let mut state = RequestMetricsState::default();
        hydrate_request_metrics_state(&mut state, &snapshot).expect("hydrate state");
        let prepared = prepare_request_metrics(&state, &snapshot).expect("prepare metrics");

        assert!(prepared.previous_request_hash.is_some());
        assert_eq!(prepared.first_divergence_byte, None);
        assert_eq!(
            prepared.reused_prefix_bytes_estimate,
            Some(prepared.cacheable_prompt_bytes)
        );
        assert!(prepared.reused_prefix_bytes_estimate.unwrap_or_default() > 0);
    }

    #[test]
    fn estimated_metrics_stay_close_to_official_for_repeated_identical_request() {
        let session_messages = vec![AgentMessage::User(UserMessage {
            content: vec![ContentBlock::Text {
                text: "summarize the cache warmup plan".to_string(),
            }],
            timestamp: 0,
        })];
        let snapshot = parity_test_snapshot(
            bb_core::agent_session::messages_to_provider(&session_messages),
            vec![],
        );
        let mut state = RequestMetricsState::default();
        hydrate_request_metrics_state(&mut state, &snapshot).expect("seed state");

        let prepared =
            prepare_request_metrics(&state, &snapshot).expect("prepare repeated request");
        let prompt_token_total = 240;
        let estimated_read = normalized_estimated_cache_read_tokens(&prepared, prompt_token_total);
        assert!(estimated_read > 0);

        let official_read = estimated_read.saturating_sub(4);
        let official_usage = resolve_cache_usage(
            &prepared,
            &response_usage_with_source(
                prompt_token_total,
                official_read,
                24,
                CacheMetricsSource::Official,
            ),
        );
        let estimated_usage = resolve_cache_usage(
            &prepared,
            &response_usage_with_source(
                prompt_token_total,
                official_read,
                24,
                CacheMetricsSource::Estimated,
            ),
        );

        let identity = RequestMetricsIdentity {
            session_id: "session".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            turn_index: 2,
        };
        let timing = RequestMetricsTiming {
            request_started_at_ms: 0,
            first_stream_event_at_ms: Some(10),
            first_text_delta_at_ms: Some(20),
            finished_at_ms: 40,
            total_latency_ms: 40,
            tool_wait_ms: 0,
            resume_latency_ms: None,
        };
        let official_metrics = build_final_request_metrics(
            prepared.clone(),
            &identity,
            &RequestMutationFlags::default(),
            &timing,
            &official_usage,
        );
        let estimated_metrics = build_final_request_metrics(
            prepared,
            &identity,
            &RequestMutationFlags::default(),
            &timing,
            &estimated_usage,
        );

        assert_eq!(
            official_metrics.cache_metrics_source,
            CacheMetricsSource::Official
        );
        assert_eq!(
            estimated_metrics.cache_metrics_source,
            CacheMetricsSource::Estimated
        );
        assert_eq!(official_metrics.estimated_cache_read_tokens, None);
        assert_eq!(
            estimated_metrics.provider_cache_read_tokens,
            Some(official_read)
        );
        assert_eq!(
            estimated_metrics.estimated_cache_read_tokens,
            Some(estimated_read)
        );
        assert_estimate_close_to_official(&estimated_metrics, &official_metrics, 4, 2.5);
    }

    #[test]
    fn estimated_metrics_stay_close_to_official_with_tools_and_history() {
        let snapshot = parity_test_snapshot(
            vec![
                json!({"role": "user", "content": "fetch repo data"}),
                json!({
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "web_fetch",
                            "arguments": "{\"url\":\"https://example.com\"}"
                        }
                    }]
                }),
                json!({
                    "role": "tool",
                    "tool_call_id": "call_1",
                    "content": "{\"ok\":true,\"stars\":42}"
                }),
                json!({"role": "user", "content": "now explain the result concisely"}),
            ],
            vec![json!({
                "type": "function",
                "function": {
                    "name": "web_fetch",
                    "description": "Fetch a URL",
                    "parameters": {
                        "type": "object",
                        "properties": {"url": {"type": "string"}},
                        "required": ["url"]
                    }
                }
            })],
        );
        let mut state = RequestMetricsState::default();
        hydrate_request_metrics_state(&mut state, &snapshot).expect("seed state");

        let prepared =
            prepare_request_metrics(&state, &snapshot).expect("prepare repeated request");
        let prompt_token_total = 320;
        let estimated_read = normalized_estimated_cache_read_tokens(&prepared, prompt_token_total);
        assert!(estimated_read > 0);

        let official_read = estimated_read.saturating_sub(8);
        let official_usage = resolve_cache_usage(
            &prepared,
            &response_usage_with_source(
                prompt_token_total,
                official_read,
                31,
                CacheMetricsSource::Official,
            ),
        );
        let estimated_usage = resolve_cache_usage(
            &prepared,
            &response_usage_with_source(
                prompt_token_total,
                official_read,
                31,
                CacheMetricsSource::Estimated,
            ),
        );

        let identity = RequestMetricsIdentity {
            session_id: "session".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            turn_index: 3,
        };
        let timing = RequestMetricsTiming {
            request_started_at_ms: 0,
            first_stream_event_at_ms: Some(12),
            first_text_delta_at_ms: Some(22),
            finished_at_ms: 48,
            total_latency_ms: 48,
            tool_wait_ms: 0,
            resume_latency_ms: None,
        };
        let official_metrics = build_final_request_metrics(
            prepared.clone(),
            &identity,
            &RequestMutationFlags::default(),
            &timing,
            &official_usage,
        );
        let estimated_metrics = build_final_request_metrics(
            prepared,
            &identity,
            &RequestMutationFlags::default(),
            &timing,
            &estimated_usage,
        );

        assert_estimate_close_to_official(&estimated_metrics, &official_metrics, 8, 2.5);
    }

    #[test]
    fn tracker_wraps_state_prepare_and_commit() {
        let snapshot = RequestMetricsSnapshot {
            system_prompt: "system".to_string(),
            provider_messages: vec![serde_json::json!({ "role": "user", "content": "hello" })],
            tool_definitions: vec![],
            extra_tool_definitions: vec![],
            model: "gpt-5".to_string(),
            max_tokens: Some(64),
            stream: true,
            thinking: Some("medium".to_string()),
        };
        let mut tracker = RequestMetricsTracker::new();
        tracker.increment_context_epoch();

        let prepared = tracker.prepare(&snapshot).expect("prepare");
        assert_eq!(prepared.context_epoch, 1);
        tracker.commit(&prepared);
        assert_eq!(
            tracker.state().last_request_hash.as_deref(),
            Some(prepared.full_request_hash.as_str())
        );
    }
}
