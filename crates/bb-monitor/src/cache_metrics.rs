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
