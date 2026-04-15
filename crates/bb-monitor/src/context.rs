use serde::{Deserialize, Serialize};

use crate::usage::ContextWindowStatus;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeContextUsage {
    pub tokens: Option<u64>,
    pub percent: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextResolutionInput {
    pub runtime_usage: Option<RuntimeContextUsage>,
    pub active_path_tokens: Option<u64>,
    pub has_contextful_active_path: bool,
    pub context_window: u64,
    pub auto_compaction: bool,
    pub suppress_runtime_usage: bool,
}

pub fn resolve_context_window_status(input: &ContextResolutionInput) -> ContextWindowStatus {
    let unknown = || ContextWindowStatus {
        context_window: input.context_window,
        used_tokens: None,
        used_percent: None,
        auto_compaction: input.auto_compaction,
    };
    let from_tokens = |tokens| ContextWindowStatus {
        context_window: input.context_window,
        used_tokens: Some(tokens),
        used_percent: None,
        auto_compaction: input.auto_compaction,
    };
    let from_percent = |percent| ContextWindowStatus {
        context_window: input.context_window,
        used_tokens: None,
        used_percent: Some(percent),
        auto_compaction: input.auto_compaction,
    };

    if input.suppress_runtime_usage {
        return unknown();
    }

    if let Some(runtime_usage) = &input.runtime_usage {
        if let Some(runtime_tokens) = runtime_usage.tokens {
            if runtime_tokens == 0 {
                if let Some(estimated_tokens) =
                    input.active_path_tokens.filter(|tokens| *tokens > 0)
                {
                    return from_tokens(estimated_tokens);
                }
                if input.has_contextful_active_path && input.active_path_tokens.is_none() {
                    return unknown();
                }
            }
            return from_tokens(runtime_tokens);
        }
        if let Some(percent) = runtime_usage.percent {
            if percent == 0 {
                if let Some(estimated_tokens) =
                    input.active_path_tokens.filter(|tokens| *tokens > 0)
                {
                    return from_tokens(estimated_tokens);
                }
                if input.has_contextful_active_path && input.active_path_tokens.is_none() {
                    return unknown();
                }
            }
            return from_percent(percent as f64);
        }
        return unknown();
    }

    if let Some(tokens) = input.active_path_tokens {
        return from_tokens(tokens);
    }

    unknown()
}

#[cfg(test)]
mod tests {
    use super::{ContextResolutionInput, RuntimeContextUsage, resolve_context_window_status};
    use crate::formatting::render_context_window_status;

    #[test]
    fn suppressing_runtime_usage_yields_unknown_context() {
        let status = resolve_context_window_status(&ContextResolutionInput {
            runtime_usage: Some(RuntimeContextUsage {
                tokens: Some(120_000),
                percent: Some(44),
            }),
            active_path_tokens: Some(120_000),
            has_contextful_active_path: true,
            context_window: 272_000,
            auto_compaction: true,
            suppress_runtime_usage: true,
        });

        assert_eq!(render_context_window_status(&status), "?/272k (auto)");
    }

    #[test]
    fn prefers_active_path_estimate_over_zero_runtime_usage() {
        let status = resolve_context_window_status(&ContextResolutionInput {
            runtime_usage: Some(RuntimeContextUsage {
                tokens: Some(0),
                percent: Some(0),
            }),
            active_path_tokens: Some(120_000),
            has_contextful_active_path: true,
            context_window: 272_000,
            auto_compaction: true,
            suppress_runtime_usage: false,
        });

        assert_eq!(render_context_window_status(&status), "44.1%/272k (auto)");
    }

    #[test]
    fn shows_unknown_when_zero_runtime_usage_has_no_estimate() {
        let status = resolve_context_window_status(&ContextResolutionInput {
            runtime_usage: Some(RuntimeContextUsage {
                tokens: Some(0),
                percent: Some(0),
            }),
            active_path_tokens: None,
            has_contextful_active_path: true,
            context_window: 272_000,
            auto_compaction: true,
            suppress_runtime_usage: false,
        });

        assert_eq!(render_context_window_status(&status), "?/272k (auto)");
    }

    #[test]
    fn preserves_valid_runtime_percent_when_present() {
        let status = resolve_context_window_status(&ContextResolutionInput {
            runtime_usage: Some(RuntimeContextUsage {
                tokens: None,
                percent: Some(75),
            }),
            active_path_tokens: Some(120_000),
            has_contextful_active_path: true,
            context_window: 272_000,
            auto_compaction: false,
            suppress_runtime_usage: false,
        });

        assert_eq!(render_context_window_status(&status), "75.0%/272k");
    }
}
