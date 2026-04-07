use bb_core::types::ContentBlock;
use serde_json::Value;

use crate::syntax;

use super::helpers::{
    collapse_preview_line, preview_text_lines, replace_tabs, shorten_path, text_output,
};

fn format_duration_ms(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{:.1}m", ms as f64 / 60_000.0)
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

pub fn format_tool_result_content(
    name: &str,
    content: &[ContentBlock],
    details: Option<Value>,
    artifact_path: Option<String>,
    is_error: bool,
    expanded: bool,
) -> String {
    let duration_line = details
        .as_ref()
        .and_then(|details| details.get("durationMs"))
        .and_then(|value| value.as_u64())
        .map(|ms| format!("duration: {}", format_duration_ms(ms)));

    let mut lines = match name {
        "read" => render_read_result(content, details.as_ref(), expanded),
        "write" => render_write_result(details.as_ref()),
        "edit" => render_edit_result(content, details.as_ref()),
        "bash" => render_bash_result(content, details.as_ref(), expanded),
        "ls" => render_list_result(content, details.as_ref(), expanded),
        "grep" => render_grep_result(content, details.as_ref(), expanded),
        "find" => render_find_result(content, details.as_ref(), expanded),
        "web_search" => render_web_search_result(content, details.as_ref(), expanded),
        "web_fetch" | "browser_fetch" => {
            render_web_fetch_result(content, details.as_ref(), expanded)
        }
        _ => render_default_result(content, expanded),
    };

    if let Some(duration_line) = duration_line {
        lines.insert(0, duration_line);
    }

    if let Some(details) = details {
        let rendered =
            serde_json::to_string_pretty(&details).unwrap_or_else(|_| details.to_string());
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

pub fn collapsed_tool_summary_with_count(name: &str, count: usize) -> Option<String> {
    let (verb, singular, plural) = match name {
        "read" => ("Read", "file", "files"),
        "grep" => ("Searched", "file", "files"),
        "find" | "ls" => ("Listed", "directory", "directories"),
        "bash" => ("Ran", "command", "commands"),
        "write" => ("Wrote", "file", "files"),
        "edit" => ("Edited", "file", "files"),
        _ => return None,
    };
    let noun = if count == 1 { singular } else { plural };
    Some(format!(
        "{verb} {count} {noun} (click or use Ctrl+Shift+O to enter tool expand mode)"
    ))
}

fn render_read_result(
    content: &[ContentBlock],
    details: Option<&Value>,
    expanded: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut file_path = String::new();
    if let Some(details) = details {
        let path = details
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let start = details
            .get("startLine")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
        let end = details
            .get("endLine")
            .and_then(|v| v.as_u64())
            .unwrap_or(start);
        let total = details
            .get("totalLines")
            .and_then(|v| v.as_u64())
            .unwrap_or(end);
        if !path.is_empty() {
            lines.push(format!(
                "read {} lines {start}-{end} / {total}",
                shorten_path(&path)
            ));
            file_path = path;
        }
    }

    let raw = text_output(content);
    let lang = syntax::language_from_path(&file_path);

    if lang.is_some() && !raw.trim().is_empty() {
        let highlighted = syntax::highlight_code(&raw, lang);
        let max_lines = if expanded { 120 } else { 3 };
        let total = highlighted.len();
        for line in highlighted.into_iter().take(max_lines) {
            lines.push(replace_tabs(&line));
        }
        if total > max_lines {
            lines.push(format!(
                "... ({} more lines; click or use Ctrl+Shift+O to enter tool expand mode)",
                total - max_lines
            ));
        }
    } else {
        lines.extend(preview_text_lines(
            &raw,
            if expanded { 120 } else { 3 },
            expanded,
        ));
    }
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
        lines.push(format!(
            "applied {applied}/{total} edit(s) to {}",
            shorten_path(path)
        ));
        if let Some(diff) = details.get("diff").and_then(|v| v.as_str()) {
            lines.extend(diff.lines().map(str::to_string));
            return lines;
        }
    }
    lines.extend(preview_text_lines(&text_output(content), 80, true));
    lines
}

fn render_bash_result(
    content: &[ContentBlock],
    details: Option<&Value>,
    expanded: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let exit = details
            .get("exitCode")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        let truncated = details
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let cancelled = details
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mut flags = Vec::new();
        if truncated {
            flags.push("truncated");
        }
        if cancelled {
            flags.push("cancelled");
        }
        if exit != 0 || !flags.is_empty() {
            let suffix = if flags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", flags.join(", "))
            };
            lines.push(format!("exit code: {exit}{suffix}"));
        }
    }
    lines.extend(preview_text_lines(
        &text_output(content),
        if expanded { 120 } else { 3 },
        expanded,
    ));
    lines
}

fn render_list_result(
    content: &[ContentBlock],
    details: Option<&Value>,
    expanded: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let count = details
            .get("entryCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let truncated = details
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let suffix = if truncated { " (truncated)" } else { "" };
        lines.push(format!(
            "{count} entr{} shown{suffix}",
            if count == 1 { "y" } else { "ies" }
        ));
    }
    lines.extend(preview_text_lines(
        &text_output(content),
        if expanded { 120 } else { 3 },
        expanded,
    ));
    lines
}

fn render_grep_result(
    content: &[ContentBlock],
    details: Option<&Value>,
    expanded: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let count = details
            .get("matchCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        lines.push(format!("{count} match(es)"));
    }
    lines.extend(preview_text_lines(
        &text_output(content),
        if expanded { 120 } else { 3 },
        expanded,
    ));
    lines
}

fn render_find_result(
    content: &[ContentBlock],
    details: Option<&Value>,
    expanded: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(details) = details {
        let count = details
            .get("matchCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        lines.push(format!("{count} file(s)"));
    }
    lines.extend(preview_text_lines(
        &text_output(content),
        if expanded { 120 } else { 3 },
        expanded,
    ));
    lines
}

fn render_web_search_result(
    content: &[ContentBlock],
    details: Option<&Value>,
    expanded: bool,
) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(details) = details {
        let backend = details
            .get("backend")
            .and_then(|v| v.as_str())
            .unwrap_or("web-search");
        let hit_count = details
            .get("hitCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let cache_hit = details
            .get("cacheHit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let query = details.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let cache_suffix = if cache_hit { " [cached]" } else { "" };
        if !query.is_empty() {
            lines.push(format!("query: \"{query}\""));
        }
        lines.push(format!("{hit_count} result(s) via {backend}{cache_suffix}"));

        if let Some(summary) = extract_web_search_summary(content)
            && !summary.trim().is_empty()
        {
            lines.push(String::new());
            lines.push("summary:".to_string());
            lines.extend(preview_text_lines(
                &summary,
                if expanded { 12 } else { 3 },
                expanded,
            ));
        }

        let links = collect_web_search_links(details);
        if !links.is_empty() {
            lines.push(String::new());
            lines.push("links:".to_string());
            let max_links = if expanded { 8 } else { 3 };
            for (title, url) in links.iter().take(max_links) {
                lines.push(format!("- {} — {}", collapse_preview_line(title, 90), url));
            }
            if links.len() > max_links {
                lines.push(format!(
                    "... ({} more link(s); click or use Ctrl+Shift+O to enter tool expand mode)",
                    links.len() - max_links
                ));
            }
        }

        return lines;
    }

    render_default_result(content, expanded)
}

fn render_web_fetch_result(
    content: &[ContentBlock],
    details: Option<&Value>,
    expanded: bool,
) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(details) = details {
        let final_url = details
            .get("finalUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let title = details.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let content_type = details
            .get("contentType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let extraction = details
            .get("extractionSource")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let truncated = details
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let citation = details
            .get("citationMarkdown")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !title.is_empty() {
            lines.push(format!("title: {title}"));
        }
        if !final_url.is_empty() {
            lines.push(format!("url: {final_url}"));
        }
        if !content_type.is_empty() || !extraction.is_empty() {
            let mut meta = Vec::new();
            if !content_type.is_empty() {
                meta.push(content_type.to_string());
            }
            if !extraction.is_empty() {
                meta.push(format!("extraction={extraction}"));
            }
            if truncated {
                meta.push("truncated".to_string());
            }
            lines.push(meta.join(" | "));
        }

        let body = extract_web_fetch_body(content);
        if !body.trim().is_empty() {
            lines.push(String::new());
            lines.push("content:".to_string());
            lines.extend(preview_text_lines(
                &body,
                if expanded { 16 } else { 4 },
                expanded,
            ));
        }

        if !citation.is_empty() {
            lines.push(String::new());
            lines.push("citation:".to_string());
            lines.push(citation.to_string());
        }

        return lines;
    }

    render_default_result(content, expanded)
}

fn render_default_result(content: &[ContentBlock], expanded: bool) -> Vec<String> {
    preview_text_lines(
        &text_output(content),
        if expanded { 120 } else { 3 },
        expanded,
    )
}

fn extract_web_search_summary(content: &[ContentBlock]) -> Option<String> {
    let text = text_output(content);
    let summary = text
        .split("\n\nLinks:\n")
        .next()
        .unwrap_or("")
        .split("\n\nSummary:\n")
        .nth(1)
        .unwrap_or("")
        .trim()
        .to_string();
    (!summary.is_empty()).then_some(summary)
}

fn collect_web_search_links(details: &Value) -> Vec<(String, String)> {
    let mut links = Vec::new();
    let Some(results) = details.get("results").and_then(|v| v.as_array()) else {
        return links;
    };

    for chunk in results {
        if chunk.get("kind").and_then(|v| v.as_str()) != Some("Hits") {
            continue;
        }
        let Some(content) = chunk.get("content").and_then(|v| v.as_array()) else {
            continue;
        };
        for hit in content {
            let title = hit
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let url = hit
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !title.is_empty() && !url.is_empty() {
                links.push((title, url));
            }
        }
    }

    links
}

fn extract_web_fetch_body(content: &[ContentBlock]) -> String {
    let text = text_output(content);
    let after_separator = text.split("\n---\n").nth(1).unwrap_or(text.as_str());
    after_separator
        .split("\n\nCitation:\n")
        .next()
        .unwrap_or(after_separator)
        .trim()
        .to_string()
}
