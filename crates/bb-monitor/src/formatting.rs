use crate::usage::{ContextWindowStatus, UsageTotals};

pub fn format_compact_tokens(count: u64) -> String {
    if count < 1_000 {
        count.to_string()
    } else if count < 10_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else if count < 1_000_000 {
        format!("{}k", (count as f64 / 1_000.0).round() as u64)
    } else if count < 10_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else {
        format!("{}M", (count as f64 / 1_000_000.0).round() as u64)
    }
}

pub fn format_u64_with_commas(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (idx, ch) in digits.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

pub fn format_context_percent(percent: f64, context_window: u64, suffix: &str) -> String {
    format!(
        "{percent:.1}%/{}{}",
        format_compact_tokens(context_window),
        suffix
    )
}

pub fn format_context_from_tokens(tokens: u64, context_window: u64, suffix: &str) -> String {
    if context_window == 0 {
        return format_unknown_context(context_window, suffix);
    }
    format_context_percent(
        (tokens as f64 / context_window as f64) * 100.0,
        context_window,
        suffix,
    )
}

pub fn format_unknown_context(context_window: u64, suffix: &str) -> String {
    format!("?/{}{}", format_compact_tokens(context_window), suffix)
}

pub fn render_context_window_status(context: &ContextWindowStatus) -> String {
    let suffix = if context.auto_compaction {
        " (auto)"
    } else {
        ""
    };
    match (context.used_tokens, context.used_percent) {
        (Some(tokens), _) => format_context_from_tokens(tokens, context.context_window, suffix),
        (None, Some(percent)) => format_context_percent(percent, context.context_window, suffix),
        (None, None) => format_unknown_context(context.context_window, suffix),
    }
}

/// Render the compact usage text currently used in BB-Agent footers and other
/// monitor surfaces, without depending on TUI rendering code.
pub fn render_footer_usage_text(
    usage: &UsageTotals,
    is_subscription: bool,
    context: &ContextWindowStatus,
) -> String {
    let mut parts = Vec::new();
    if usage.input_tokens > 0 {
        parts.push(format!("↑{}", format_compact_tokens(usage.input_tokens)));
    }
    if usage.output_tokens > 0 {
        parts.push(format!("↓{}", format_compact_tokens(usage.output_tokens)));
    }
    if usage.cache_read_tokens > 0 {
        parts.push(format!(
            "R{}",
            format_compact_tokens(usage.cache_read_tokens)
        ));
    }
    if usage.cache_write_tokens > 0 {
        parts.push(format!(
            "W{}",
            format_compact_tokens(usage.cache_write_tokens)
        ));
    }
    if usage.total_cost > 0.0 || is_subscription {
        let sub = if is_subscription { " (sub)" } else { "" };
        parts.push(format!("${:.3}{sub}", usage.total_cost));
    }
    parts.push(render_context_window_status(context));

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        format_compact_tokens, format_context_from_tokens, format_context_percent,
        format_u64_with_commas, format_unknown_context, render_context_window_status,
        render_footer_usage_text,
    };
    use crate::usage::{ContextWindowStatus, UsageTotals};

    #[test]
    fn compact_token_format_matches_existing_bb_footer_conventions() {
        assert_eq!(format_compact_tokens(0), "0");
        assert_eq!(format_compact_tokens(999), "999");
        assert_eq!(format_compact_tokens(1_500), "1.5k");
        assert_eq!(format_compact_tokens(754_000), "754k");
        assert_eq!(format_compact_tokens(272_000), "272k");
        assert_eq!(format_compact_tokens(13_000_000), "13M");
        assert_eq!(format_compact_tokens(275_000_000), "275M");
    }

    #[test]
    fn comma_formatter_matches_existing_session_info_strings() {
        assert_eq!(format_u64_with_commas(0), "0");
        assert_eq!(format_u64_with_commas(12), "12");
        assert_eq!(format_u64_with_commas(1234), "1,234");
        assert_eq!(format_u64_with_commas(27_064_604), "27,064,604");
    }

    #[test]
    fn context_formatters_match_current_bb_strings() {
        assert_eq!(format_context_percent(0.0, 272_000, ""), "0.0%/272k");
        assert_eq!(
            format_context_from_tokens(0, 272_000, " (auto)"),
            "0.0%/272k (auto)"
        );
        assert_eq!(format_unknown_context(272_000, " (auto)"), "?/272k (auto)");
    }

    #[test]
    fn renders_context_status_text() {
        let context = ContextWindowStatus {
            context_window: 272_000,
            used_tokens: None,
            used_percent: Some(75.9),
            auto_compaction: true,
        };
        assert_eq!(render_context_window_status(&context), "75.9%/272k (auto)");
    }

    #[test]
    fn renders_compact_footer_usage_text() {
        let usage = UsageTotals {
            input_tokens: 13_000_000,
            output_tokens: 754_000,
            cache_read_tokens: 275_000_000,
            cache_write_tokens: 0,
            total_tokens: 288_754_000,
            total_cost: 112.751,
        };
        let context = ContextWindowStatus {
            context_window: 272_000,
            used_tokens: None,
            used_percent: Some(75.9),
            auto_compaction: true,
        };

        assert_eq!(
            render_footer_usage_text(&usage, true, &context),
            "↑13M ↓754k R275M $112.751 (sub) 75.9%/272k (auto)"
        );
    }
}
