use serde::{Deserialize, Serialize};

/// Backend aggregate for usage/cost totals collected across a session or turn.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub total_cost: f64,
}

impl UsageTotals {
    /// Return the stored total token count when present, otherwise recompute it
    /// from directional usage buckets.
    pub fn effective_total_tokens(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens
                + self.output_tokens
                + self.cache_read_tokens
                + self.cache_write_tokens
        }
    }
}

/// Snapshot of context-window usage for display or logging.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextWindowStatus {
    pub context_window: u64,
    pub used_tokens: Option<u64>,
    pub used_percent: Option<f64>,
    pub auto_compaction: bool,
}
