use bb_core::types::ContentBlock;

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
            // Find the first non-empty, non-comment line as the display command.
            // If all lines are comments, fall back to the first non-empty line.
            let first_real = full_cmd
                .lines()
                .find(|line| {
                    let t = line.trim();
                    !t.is_empty() && !t.starts_with('#')
                })
                .or_else(|| full_cmd.lines().find(|line| !line.trim().is_empty()))
                .unwrap_or_default()
                .trim();
            // Truncate very long commands for the header
            if first_real.len() > 120 {
                format!("{}…", &first_real[..119])
            } else {
                first_real.to_string()
            }
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
