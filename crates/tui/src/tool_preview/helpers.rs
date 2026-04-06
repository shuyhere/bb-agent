use bb_core::types::ContentBlock;
use serde_json::Value;

pub(super) fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub(crate) fn extract_tool_arg_string_relaxed(raw_args: &str, key: &str) -> Option<String> {
    if let Ok(args) = serde_json::from_str::<Value>(raw_args)
        && let Some(value) = arg_str(&args, key)
    {
        return Some(value);
    }

    extract_jsonish_string_field(raw_args, key)
}

fn extract_jsonish_string_field(raw: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = raw.find(&needle)?;
    let mut rest = &raw[start + needle.len()..];
    rest = rest.trim_start();
    rest = rest.strip_prefix(':')?;
    rest = rest.trim_start();
    rest = rest.strip_prefix('"')?;

    let mut out = String::new();
    let mut escaped = false;
    let mut closed = false;

    for ch in rest.chars() {
        if escaped {
            match ch {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '"' => {
                closed = true;
                break;
            }
            other => out.push(other),
        }
    }

    if closed || !out.is_empty() {
        Some(out)
    } else {
        None
    }
}

pub(super) fn shorten_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    if let Ok(home) = std::env::var("HOME")
        && path.starts_with(&home)
    {
        return format!("~{}", &path[home.len()..]);
    }
    path.to_string()
}

pub(super) fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
}

pub(super) fn summarize_inline(text: &str, max_chars: usize) -> String {
    let text = text.replace('\n', "\\n");
    if text.chars().count() <= max_chars {
        text
    } else {
        let prefix: String = text.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{prefix}…")
    }
}

pub(super) fn collapse_preview_line(line: &str, max_chars: usize) -> String {
    summarize_inline(&replace_tabs(line), max_chars)
}

pub(super) fn text_output(content: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => parts.push(text.clone()),
            ContentBlock::Image { mime_type, .. } => parts.push(format!("[image: {mime_type}]")),
        }
    }
    parts.join("\n")
}

pub(super) fn preview_text_lines(text: &str, max_lines: usize, expanded: bool) -> Vec<String> {
    const COLLAPSED_MAX_CHARS_PER_LINE: usize = 120;

    if text.trim().is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        if expanded {
            return lines.iter().map(|line| replace_tabs(line)).collect();
        }
        return lines
            .iter()
            .map(|line| collapse_preview_line(line, COLLAPSED_MAX_CHARS_PER_LINE))
            .collect();
    }

    if !expanded {
        let mut out = Vec::new();
        for line in lines.iter().take(max_lines) {
            out.push(collapse_preview_line(line, COLLAPSED_MAX_CHARS_PER_LINE));
        }
        out.push(format!(
            "... ({} more lines; click or use Ctrl+Shift+O to enter tool expand mode)",
            lines.len() - max_lines
        ));
        return out;
    }

    let head = (max_lines / 2).max(1);
    let tail = (max_lines.saturating_sub(head + 1)).max(1);
    let hidden = lines.len().saturating_sub(head + tail);
    let mut out = Vec::new();
    for line in lines.iter().take(head) {
        out.push(replace_tabs(line));
    }
    out.push(format!("… output truncated ({} lines hidden)", hidden));
    for line in lines.iter().skip(lines.len().saturating_sub(tail)) {
        out.push(replace_tabs(line));
    }
    out
}
