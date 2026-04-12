use std::sync::OnceLock;

use bb_core::types::ContentBlock;
use regex::{Captures, Regex};

fn is_bash_prelude_line(line: &str) -> bool {
    matches!(
        line.trim(),
        "set -e"
            | "set -u"
            | "set -eu"
            | "set -ue"
            | "set -o pipefail"
            | "set -eo pipefail"
            | "set -ueo pipefail"
            | "set -euo pipefail"
            | "set -uo pipefail"
    )
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !value.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_secret_env_name(name: &str) -> bool {
    let upper = name.replace('-', "_").to_ascii_uppercase();
    upper.contains("API_KEY")
        || upper.ends_with("_KEY")
        || upper.ends_with("_TOKEN")
        || upper.contains("ACCESS_TOKEN")
        || upper.contains("REFRESH_TOKEN")
        || upper.contains("SECRET")
        || upper.contains("PASSWORD")
        || upper.contains("PASSWD")
}

fn strip_leading_env_assignments(line: &str) -> String {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    let first_non_assignment = tokens
        .iter()
        .position(|token| !looks_like_env_assignment(token));
    match first_non_assignment {
        Some(index) if index > 0 => tokens[index..].join(" "),
        _ => line.trim().to_string(),
    }
}

fn secret_assignment_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)=([^\s]+)").expect("valid regex"))
}

fn authorization_header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)Authorization:\s*(Bearer|token)\s+[^\s"']+"#)
            .expect("valid regex")
    })
}

fn bearer_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)\bBearer\s+[^\s"']+"#).expect("valid regex"))
}

fn redact_bash_title_line(line: &str) -> String {
    let stripped = strip_leading_env_assignments(line);
    let redacted_assignments = secret_assignment_regex().replace_all(&stripped, |caps: &Captures| {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        if is_secret_env_name(name) {
            format!("{name}=[REDACTED]")
        } else {
            caps.get(0)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        }
    });
    let redacted_auth = authorization_header_regex().replace_all(
        redacted_assignments.as_ref(),
        |caps: &Captures| {
            let scheme = caps.get(1).map(|m| m.as_str()).unwrap_or("Bearer");
            format!("Authorization: {scheme} [REDACTED]")
        },
    );
    bearer_token_regex()
        .replace_all(redacted_auth.as_ref(), "Bearer [REDACTED]")
        .into_owned()
}

pub fn format_tool_call_title(name: &str, raw_args: &str) -> String {
    let parsed_args = serde_json::from_str::<serde_json::Value>(raw_args).ok();

    let inner = match name {
        "bash" => {
            let full_cmd = parsed_args
                .as_ref()
                .and_then(|args| args.get("command"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or_else(|| {
                    crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "command")
                })
                .unwrap_or_default();
            let timeout = parsed_args
                .as_ref()
                .and_then(|args| args.get("timeout"))
                .and_then(|value| value.as_f64());
            // Find the first non-empty, non-comment line as the display command.
            // If all lines are comments, fall back to the first non-empty line.
            let first_real = full_cmd
                .lines()
                .find(|line| {
                    let t = line.trim();
                    !t.is_empty() && !t.starts_with('#') && !is_bash_prelude_line(t)
                })
                .or_else(|| {
                    full_cmd.lines().find(|line| {
                        let t = line.trim();
                        !t.is_empty() && !t.starts_with('#')
                    })
                })
                .or_else(|| full_cmd.lines().find(|line| !line.trim().is_empty()))
                .unwrap_or_default()
                .trim();
            let redacted = redact_bash_title_line(first_real);
            let mut display = if redacted.len() > 120 {
                format!("{}…", &redacted[..119])
            } else {
                redacted
            };
            if let Some(timeout) = timeout {
                let timeout_text = if timeout.fract() == 0.0 {
                    format!(" timeout={}s", timeout as i64)
                } else {
                    format!(" timeout={timeout}s")
                };
                display.push_str(&timeout_text);
            }
            display
        }
        "read" => {
            let path = crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "path")
                .unwrap_or_default();
            let offset = parsed_args
                .as_ref()
                .and_then(|args| args.get("offset"))
                .and_then(|value| value.as_u64());
            let limit = parsed_args
                .as_ref()
                .and_then(|args| args.get("limit"))
                .and_then(|value| value.as_u64());
            let mut line_suffix = String::new();
            if offset.is_some() || limit.is_some() {
                let start = offset.unwrap_or(1);
                if let Some(limit) = limit {
                    let end = start.saturating_add(limit).saturating_sub(1);
                    line_suffix = format!(":{start}-{end}");
                } else {
                    line_suffix = format!(":{start}");
                }
            }
            format!("{}{line_suffix}", shorten_display_path(&path))
        }
        "write" | "edit" => crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "path")
            .map(|path| shorten_display_path(&path))
            .unwrap_or_default(),
        "ls" => {
            let path_owned = crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "path")
                .unwrap_or_else(|| ".".to_string());
            let path = path_owned.as_str();
            let limit = parsed_args
                .as_ref()
                .and_then(|args| args.get("limit"))
                .and_then(|value| value.as_u64());
            match limit {
                Some(limit) => format!("{} limit={limit}", shorten_display_path(path)),
                None => shorten_display_path(path),
            }
        }
        "grep" => {
            let pattern_owned =
                crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "pattern")
                    .unwrap_or_default();
            let path_owned = crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "path")
                .unwrap_or_else(|| ".".to_string());
            let pattern = pattern_owned.as_str();
            let path = path_owned.as_str();
            let glob_owned = crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "glob");
            let glob = glob_owned.as_deref();
            let mut text = if pattern.is_empty() {
                shorten_display_path(path)
            } else {
                format!("/{pattern}/ {}", shorten_display_path(path))
            };
            if let Some(glob) = glob.filter(|glob| !glob.is_empty()) {
                text.push_str(&format!(" {glob}"));
            }
            text
        }
        "find" => {
            let pattern_owned =
                crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "pattern")
                    .unwrap_or_default();
            let path_owned = crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "path")
                .unwrap_or_else(|| ".".to_string());
            let pattern = pattern_owned.as_str();
            let path = path_owned.as_str();
            if pattern.is_empty() {
                shorten_display_path(path)
            } else {
                format!("{pattern} {}", shorten_display_path(path))
            }
        }
        "web_search" => {
            let query = crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "query")
                .unwrap_or_default();
            if query.chars().count() > 120 {
                let prefix: String = query.chars().take(119).collect();
                format!("\"{prefix}…\"")
            } else if query.is_empty() {
                String::new()
            } else {
                format!("\"{query}\"")
            }
        }
        "web_fetch" | "browser_fetch" => {
            crate::tool_preview::extract_tool_arg_string_relaxed(raw_args, "url")
                .map(|url| shorten_display_path(&url))
                .unwrap_or_default()
        }
        _ => String::new(),
    };

    if inner.trim().is_empty() {
        capitalize_tool_name(name).to_string()
    } else {
        format!("{}({inner})", capitalize_tool_name(name))
    }
}

fn capitalize_tool_name(name: &str) -> &'static str {
    match name {
        "bash" => "Bash",
        "read" => "Read",
        "write" => "Write",
        "edit" => "Edit",
        "ls" => "LS",
        "grep" => "Grep",
        "find" => "Find",
        "web_search" => "WebSearch",
        "web_fetch" => "WebFetch",
        "browser_fetch" => "BrowserFetch",
        _ => "Tool",
    }
}

pub(crate) fn shorten_display_path(path: &str) -> String {
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

pub fn format_tool_call_content(name: &str, raw_args: &str, expanded: bool) -> String {
    crate::tool_preview::format_tool_call_content(name, raw_args, expanded)
}

pub fn format_tool_result_content(
    name: &str,
    content: &[ContentBlock],
    details: Option<serde_json::Value>,
    artifact_path: Option<String>,
    is_error: bool,
    expanded: bool,
) -> String {
    crate::tool_preview::format_tool_result_content(
        name,
        content,
        details,
        artifact_path,
        is_error,
        expanded,
    )
}
