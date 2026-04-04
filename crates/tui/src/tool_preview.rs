use bb_core::types::ContentBlock;
use serde_json::Value;

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn shorten_path(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    if let Ok(home) = std::env::var("HOME") {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
}

fn summarize_inline(text: &str, max_chars: usize) -> String {
    let text = text.replace('\n', "\\n");
    if text.chars().count() <= max_chars {
        text
    } else {
        let prefix: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{prefix}...")
    }
}

fn bash_command_lines(args: &Value) -> Vec<String> {
    arg_str(args, "command")
        .unwrap_or_default()
        .lines()
        .map(|line| line.to_string())
        .collect()
}

pub fn format_tool_call_content(name: &str, raw_args: &str, expanded: bool) -> String {
    if raw_args.trim().is_empty() {
        return String::new();
    }

    let Ok(args) = serde_json::from_str::<Value>(raw_args) else {
        return raw_args.to_string();
    };

    let lines = match name {
        "write" => render_write_call_body(&args, expanded),
        "edit" => render_edit_call_body(&args),
        "bash" => render_bash_call_body(&args),
        "read" | "ls" | "grep" | "find" => {
            // These tools have simple args (path, pattern, etc.)
            // The title already shows the important info; no body needed.
            Vec::new()
        }
        _ => render_generic_call_body(&args),
    };

    lines.join("\n")
}

pub fn format_tool_result_content(
    name: &str,
    content: &[ContentBlock],
    details: Option<Value>,
    artifact_path: Option<String>,
    is_error: bool,
    expanded: bool,
) -> String {
    let mut lines = match name {
        "read" => render_read_result(content, details.as_ref(), expanded),
        "write" => render_write_result(details.as_ref()),
        "edit" => render_edit_result(content, details.as_ref()),
        "bash" => render_bash_result(content, details.as_ref(), expanded),
        "ls" => render_list_result(content, details.as_ref(), expanded),
        "grep" => render_grep_result(content, details.as_ref(), expanded),
        "find" => render_find_result(content, details.as_ref(), expanded),
        _ => render_default_result(content, expanded),
    };

    if let Some(details) = details {
        let rendered = serde_json::to_string_pretty(&details).unwrap_or_else(|_| details.to_string());
        if !rendered.trim().is_empty() && lines.is_empty() {
            lines.push("details:".to_string());
            lines.extend(rendered.lines().map(str::to_string));
        }
    }

    if let Some(path) = artifact_path {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("artifact: {}", shorten_path(&path)));
    }

    if lines.is_empty() {
        if is_error {
            "tool failed with no textual output".to_string()
        } else {
            "(no textual output)".to_string()
        }
    } else {
        lines.join("\n")
    }
}

fn render_generic_call_body(args: &Value) -> Vec<String> {
    let rendered = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
    if rendered == "null" || rendered == "{}" {
        Vec::new()
    } else {
        rendered.lines().map(str::to_string).collect()
    }
}

fn render_bash_call_body(args: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    let command_lines = bash_command_lines(args);
    if command_lines.is_empty() {
        if let Some(timeout) = args.get("timeout").and_then(|v| v.as_f64()) {
            lines.push(format!("timeout {timeout}s"));
        }
        return lines;
    }

    for line in command_lines {
        lines.push(replace_tabs(&line));
    }
    if let Some(timeout) = args.get("timeout").and_then(|v| v.as_f64()) {
        lines.push(format!("timeout {timeout}s"));
    }
    lines
}

fn render_edit_call_body(args: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(edits) = args.get("edits").and_then(|v| v.as_array()) {
        lines.push(format!("{} edit block(s)", edits.len()));
        for (index, edit) in edits.iter().take(3).enumerate() {
            let old_text = edit.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
            let new_text = edit.get("newText").and_then(|v| v.as_str()).unwrap_or("");
            let old_preview = summarize_inline(old_text, 60);
            let new_preview = summarize_inline(new_text, 60);
            lines.push(format!("{}. - {old_preview}", index + 1));
            lines.push(format!("   + {new_preview}"));
        }
        if edits.len() > 3 {
            lines.push(format!("... ({} more edit block(s))", edits.len() - 3));
        }
    }
    lines
}

fn render_write_call_body(args: &Value, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(content) = arg_str(args, "content") {
        let preview_lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
        let max_lines = if expanded { 120 } else { 3 };
        lines.extend(
            preview_lines
                .iter()
                .take(max_lines)
                .map(|line| replace_tabs(line)),
        );
        if preview_lines.len() > max_lines {
            lines.push(format!(
                "... ({} more lines; Ctrl+O to expand)",
                preview_lines.len() - max_lines
            ));
        }
    }
    lines
}

fn render_read_result(content: &[ContentBlock], details: Option<&Value>, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let path = details
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let start = details.get("startLine").and_then(|v| v.as_u64()).unwrap_or(1);
        let end = details.get("endLine").and_then(|v| v.as_u64()).unwrap_or(start);
        let total = details.get("totalLines").and_then(|v| v.as_u64()).unwrap_or(end);
        if !path.is_empty() {
            lines.push(format!("read {} lines {start}-{end} / {total}", shorten_path(&path)));
        }
    }
    lines.extend(preview_text_lines(&text_output(content), if expanded { 120 } else { 3 }));
    lines
}

fn render_write_result(details: Option<&Value>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let path = details.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let bytes = details.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("wrote {bytes} bytes to {}", shorten_path(path)));
    }
    lines
}

fn render_edit_result(content: &[ContentBlock], details: Option<&Value>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let path = details.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let applied = details.get("applied").and_then(|v| v.as_u64()).unwrap_or(0);
        let total = details.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("applied {applied}/{total} edit(s) to {}", shorten_path(path)));
        if let Some(diff) = details.get("diff").and_then(|v| v.as_str()) {
            lines.extend(diff.lines().map(str::to_string));
            return lines;
        }
    }
    lines.extend(preview_text_lines(&text_output(content), 80));
    lines
}

fn render_bash_result(content: &[ContentBlock], details: Option<&Value>, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let exit = details.get("exitCode").and_then(|v| v.as_i64()).unwrap_or(-1);
        let truncated = details.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
        let cancelled = details.get("cancelled").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut flags = Vec::new();
        if truncated {
            flags.push("truncated");
        }
        if cancelled {
            flags.push("cancelled");
        }
        let suffix = if flags.is_empty() {
            String::new()
        } else {
            format!(" [{}]", flags.join(", "))
        };
        lines.push(format!("exit code: {exit}{suffix}"));
    }
    lines.extend(preview_text_lines(&text_output(content), if expanded { 120 } else { 3 }));
    lines
}

fn render_list_result(content: &[ContentBlock], details: Option<&Value>, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let count = details.get("entryCount").and_then(|v| v.as_u64()).unwrap_or(0);
        let truncated = details.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false);
        let suffix = if truncated { " (truncated)" } else { "" };
        lines.push(format!("{count} entr{} shown{suffix}", if count == 1 { "y" } else { "ies" }));
    }
    lines.extend(preview_text_lines(&text_output(content), if expanded { 120 } else { 3 }));
    lines
}

fn render_grep_result(content: &[ContentBlock], details: Option<&Value>, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let count = details.get("matchCount").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("{count} match(es)"));
    }
    lines.extend(preview_text_lines(&text_output(content), if expanded { 120 } else { 3 }));
    lines
}

fn render_find_result(content: &[ContentBlock], details: Option<&Value>, expanded: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let count = details.get("matchCount").and_then(|v| v.as_u64()).unwrap_or(0);
        lines.push(format!("{count} file(s)"));
    }
    lines.extend(preview_text_lines(&text_output(content), if expanded { 120 } else { 3 }));
    lines
}

fn render_default_result(content: &[ContentBlock], expanded: bool) -> Vec<String> {
    preview_text_lines(&text_output(content), if expanded { 120 } else { 3 })
}

fn text_output(content: &[ContentBlock]) -> String {
    let mut parts = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text { text } => parts.push(text.clone()),
            ContentBlock::Image { mime_type, .. } => parts.push(format!("[image: {mime_type}]")),
        }
    }
    parts.join("\n")
}

fn preview_text_lines(text: &str, max_lines: usize) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for line in lines.iter().take(max_lines) {
        out.push(replace_tabs(line));
    }
    if lines.len() > max_lines {
        out.push(format!(
            "... ({} more lines; Ctrl+O to expand)",
            lines.len() - max_lines
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{format_tool_call_content, format_tool_result_content};
    use bb_core::types::ContentBlock;

    #[test]
    fn edit_results_keep_old_ui_wider_preview_limit() {
        let text = (1..=85).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let rendered = format_tool_result_content("edit", &[ContentBlock::Text { text }], None, None, false, false);
        assert!(rendered.contains("line 80"));
        assert!(!rendered.contains("line 81\nline 82"));
    }

    #[test]
    fn grep_call_body_is_empty_because_title_shows_details() {
        let rendered = format_tool_call_content(
            "grep",
            &serde_json::json!({"pattern":"todo","path":"/tmp","glob":"*.rs"}).to_string(),
            false,
        );
        // grep/read/ls/find tools show details in the title, not the body
        assert!(rendered.is_empty());
    }

    #[test]
    fn bash_call_body_keeps_first_line_visible() {
        let rendered = format_tool_call_content(
            "bash",
            &serde_json::json!({"command":"echo hi\nprintf done","timeout": 5.0}).to_string(),
            false,
        );
        assert!(rendered.contains("echo hi"));
        assert!(rendered.contains("printf done"));
        assert!(rendered.contains("timeout 5s"));
    }

    #[test]
    fn truncated_preview_mentions_ctrl_o_expand() {
        let text = (1..=14).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let rendered = format_tool_result_content(
            "bash",
            &[ContentBlock::Text { text }],
            None,
            None,
            false,
            false,
        );
        assert!(rendered.contains("Ctrl+O to expand"));
    }
}
