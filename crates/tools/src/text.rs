pub(crate) fn truncate_chars_trimmed(text: &str, max_chars: usize) -> String {
    text.chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

pub(crate) fn format_limited_results(
    results: &[String],
    empty_message: &str,
    limit: usize,
) -> (String, bool) {
    let total = results.len();
    let truncated = total >= limit;
    let mut text = if results.is_empty() {
        empty_message.to_string()
    } else {
        results.join("\n")
    };

    if truncated {
        text.push_str(&format!("\n\n[Results truncated at {limit} matches]"));
    }

    (text, truncated)
}

pub(crate) fn lossy_trimmed(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}
