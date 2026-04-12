use std::borrow::Cow;

fn is_builtin_tool_name(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "bash"
            | "edit"
            | "write"
            | "find"
            | "grep"
            | "ls"
            | "web_search"
            | "web_fetch"
            | "browser_fetch"
    )
}

pub fn normalize_requested_tool_name(name: &str) -> Cow<'_, str> {
    let stripped = name.strip_prefix("functions.").unwrap_or(name);
    if is_builtin_tool_name(stripped) {
        return Cow::Borrowed(stripped);
    }

    let lower = stripped.to_ascii_lowercase();
    if is_builtin_tool_name(&lower) {
        Cow::Owned(lower)
    } else {
        Cow::Borrowed(stripped)
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_requested_tool_name;

    #[test]
    fn normalizes_builtin_tool_aliases() {
        assert_eq!(normalize_requested_tool_name("bash").as_ref(), "bash");
        assert_eq!(normalize_requested_tool_name("Bash").as_ref(), "bash");
        assert_eq!(normalize_requested_tool_name("LS").as_ref(), "ls");
        assert_eq!(
            normalize_requested_tool_name("functions.read").as_ref(),
            "read"
        );
        assert_eq!(
            normalize_requested_tool_name("functions.web_fetch").as_ref(),
            "web_fetch"
        );
    }

    #[test]
    fn preserves_unknown_custom_tool_names() {
        assert_eq!(
            normalize_requested_tool_name("myCustomTool").as_ref(),
            "myCustomTool"
        );
        assert_eq!(
            normalize_requested_tool_name("functions.myCustomTool").as_ref(),
            "myCustomTool"
        );
    }
}
