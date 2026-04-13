use serde_json::Value;

use crate::ui_hints::more_lines_expand_hint;

use super::helpers::{arg_str, extract_tool_arg_string_relaxed, replace_tabs, summarize_inline};

pub fn format_tool_call_content(name: &str, raw_args: &str, expanded: bool) -> String {
    if raw_args.trim().is_empty() {
        return String::new();
    }

    let Ok(args) = serde_json::from_str::<Value>(raw_args) else {
        return match name {
            "bash" => extract_tool_arg_string_relaxed(raw_args, "command")
                .map(|command| render_bash_call_body_from_command(&command, expanded).join("\n"))
                .unwrap_or_default(),
            _ => raw_args.to_string(),
        };
    };

    let lines = match name {
        "write" => render_write_call_body(&args, expanded),
        "edit" => render_edit_call_body(&args),
        "bash" => render_bash_call_body(&args, expanded),
        "read" | "ls" | "grep" | "find" | "web_search" | "web_fetch" | "browser_fetch" => {
            Vec::new()
        }
        _ => render_generic_call_body(&args),
    };

    lines.join("\n")
}

fn render_generic_call_body(args: &Value) -> Vec<String> {
    let rendered = serde_json::to_string_pretty(args).unwrap_or_else(|_| args.to_string());
    if rendered == "null" || rendered == "{}" {
        Vec::new()
    } else {
        rendered.lines().map(str::to_string).collect()
    }
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
            lines.push(more_lines_expand_hint(preview_lines.len() - max_lines));
        }
    }
    lines
}

fn render_bash_call_body(args: &Value, expanded: bool) -> Vec<String> {
    let command = arg_str(args, "command").unwrap_or_default();
    render_bash_call_body_from_command(&command, expanded)
}

fn render_bash_call_body_from_command(command: &str, expanded: bool) -> Vec<String> {
    if command.trim().is_empty() {
        return Vec::new();
    }

    let preview_lines: Vec<String> = command.lines().map(replace_tabs).collect();
    let max_lines = if expanded { 120 } else { 8 };

    let mut lines = Vec::with_capacity(preview_lines.len().min(max_lines) + 3);
    lines.push("```bash".to_string());
    lines.extend(preview_lines.iter().take(max_lines).cloned());
    if preview_lines.len() > max_lines {
        lines.push(more_lines_expand_hint(preview_lines.len() - max_lines));
    }
    lines.push("```".to_string());
    lines
}
