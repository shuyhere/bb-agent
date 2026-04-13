use std::borrow::Cow;

pub const BUILTIN_TOOL_NAMES: &[&str] = &[
    "read",
    "bash",
    "edit",
    "write",
    "find",
    "grep",
    "ls",
    "web_search",
    "web_fetch",
    "browser_fetch",
];

fn is_builtin_tool_name(name: &str) -> bool {
    BUILTIN_TOOL_NAMES.contains(&name)
}

pub fn default_builtin_tool_names() -> Vec<String> {
    BUILTIN_TOOL_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect()
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
