use bb_core::types::ContentBlock;

pub(crate) fn format_tool_call_title(name: &str, raw_args: &str) -> String {
    let Ok(args) = serde_json::from_str::<serde_json::Value>(raw_args) else {
        return name.to_string();
    };

    match name {
        "bash" => {
            let first_line = args
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or_default()
                .trim()
                .to_string();
            if first_line.is_empty() {
                "bash".to_string()
            } else {
                format!("$ {first_line}")
            }
        }
        "read" => {
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or_default();
            let offset = args.get("offset").and_then(|value| value.as_u64());
            let limit = args.get("limit").and_then(|value| value.as_u64());
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
            if path.is_empty() {
                "read".to_string()
            } else {
                format!("read {}{line_suffix}", shorten_display_path(path))
            }
        }
        "write" | "edit" => {
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or_default();
            if path.is_empty() {
                name.to_string()
            } else {
                format!("{name} {}", shorten_display_path(path))
            }
        }
        "ls" => {
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or(".");
            let limit = args.get("limit").and_then(|value| value.as_u64());
            match limit {
                Some(limit) => format!("ls {} (limit {limit})", shorten_display_path(path)),
                None => format!("ls {}", shorten_display_path(path)),
            }
        }
        "grep" => {
            let pattern = args
                .get("pattern")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or(".");
            let glob = args.get("glob").and_then(|value| value.as_str());
            let mut title = if pattern.is_empty() {
                format!("grep {}", shorten_display_path(path))
            } else {
                format!("grep /{pattern}/ in {}", shorten_display_path(path))
            };
            if let Some(glob) = glob.filter(|glob| !glob.is_empty()) {
                title.push_str(&format!(" ({glob})"));
            }
            title
        }
        "find" => {
            let pattern = args
                .get("pattern")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let path = args.get("path").and_then(|value| value.as_str()).unwrap_or(".");
            if pattern.is_empty() {
                format!("find {}", shorten_display_path(path))
            } else {
                format!("find {pattern} in {}", shorten_display_path(path))
            }
        }
        _ => name.to_string(),
    }
}

pub(crate) fn shorten_display_path(path: &str) -> String {
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

pub(crate) fn format_tool_call_content(name: &str, raw_args: &str, expanded: bool) -> String {
    crate::tool_preview::format_tool_call_content(name, raw_args, expanded)
}

pub(crate) fn format_tool_result_content(
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
