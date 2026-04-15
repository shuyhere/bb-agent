#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixDiff {
    pub first_divergence_byte: Option<usize>,
    pub common_prefix_bytes: usize,
}

pub fn diff_prefix(previous: &str, current: &str) -> PrefixDiff {
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

pub fn estimate_tokens_from_bytes_for_model(bytes: usize, model: &str) -> u64 {
    let bytes_per_token = if model.contains("claude") { 3.45 } else { 4.0 };
    ((bytes as f64) / bytes_per_token).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::{diff_prefix, estimate_tokens_from_bytes_for_model};

    #[test]
    fn diff_prefix_reports_first_divergence_and_common_prefix() {
        let diff = diff_prefix("abcdef", "abcXYZ");
        assert_eq!(diff.common_prefix_bytes, 3);
        assert_eq!(diff.first_divergence_byte, Some(3));
    }

    #[test]
    fn diff_prefix_reports_none_for_identical_strings() {
        let diff = diff_prefix("abcdef", "abcdef");
        assert_eq!(diff.common_prefix_bytes, 6);
        assert_eq!(diff.first_divergence_byte, None);
    }

    #[test]
    fn token_estimate_uses_model_specific_byte_ratio() {
        assert_eq!(estimate_tokens_from_bytes_for_model(16, "gpt-5"), 4);
        assert_eq!(estimate_tokens_from_bytes_for_model(16, "claude-sonnet"), 5);
    }
}
