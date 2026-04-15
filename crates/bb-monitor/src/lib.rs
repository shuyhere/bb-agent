//! Shared backend monitoring helpers for BB-Agent.
//!
//! This crate is intentionally UI-agnostic: it owns metric types, cache-metric
//! vocabularies, and text-formatting helpers for usage/context summaries, but it
//! does not render TUI widgets.

pub mod cache_metrics;
pub mod usage;

pub use cache_metrics::{
    CacheMetricsSource, PreparedRequestMetrics, RequestCacheMetrics, RequestMutationFlags,
    ResolvedCacheUsage, cache_effective_utilization_pct, cache_read_hit_rate_pct,
};
pub use usage::{
    ContextWindowStatus, UsageTotals, format_compact_tokens, format_context_from_tokens,
    format_context_percent, format_unknown_context, render_footer_usage_text,
};
