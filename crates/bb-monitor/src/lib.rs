//! Shared backend monitoring helpers for BB-Agent.
//!
//! This crate is intentionally UI-agnostic: it owns metric types, cache-metric
//! vocabularies, and text-formatting helpers for usage/context summaries, but it
//! does not render TUI widgets.

pub mod cache_metrics;
pub mod context;
pub mod formatting;
pub mod request_metrics;
pub mod session;
pub mod usage;

pub use cache_metrics::{
    CacheMetricsSource, cache_effective_utilization_pct, cache_read_hit_rate_pct,
};
pub use context::{ContextResolutionInput, RuntimeContextUsage, resolve_context_window_status};
pub use formatting::{
    format_compact_tokens, format_context_from_tokens, format_context_percent,
    format_u64_with_commas, format_unknown_context, render_context_window_status,
    render_footer_usage_text,
};
pub use request_metrics::{
    PreparedRequestMetrics, RequestCacheMetrics, RequestMetricsIdentity, RequestMetricsSnapshot,
    RequestMetricsState, RequestMetricsTiming, RequestMetricsTracker, RequestMutationFlags,
    ResolvedCacheUsage, ResponseUsage, append_request_metrics_jsonl, build_final_request_metrics,
    canonical_cacheable_prompt, canonical_json_from_serializable, canonical_json_from_value,
    commit_request_metrics_state, diff_prefix, estimate_tokens_from_bytes_for_model,
    hydrate_request_metrics_state, prepare_request_metrics, resolve_cache_usage,
    write_request_metrics_jsonl,
};
pub use session::{SessionMetricsSummary, collect_session_metrics};
pub use usage::{ContextWindowStatus, UsageTotals};
