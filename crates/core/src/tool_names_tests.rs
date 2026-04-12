use crate::tool_names::normalize_requested_tool_name;

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
