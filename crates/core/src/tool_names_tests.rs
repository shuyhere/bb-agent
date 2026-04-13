use crate::agent_session_extensions::create_all_tool_definitions;
use crate::tool_names::{
    default_builtin_tool_names, normalize_requested_tool_name, BUILTIN_TOOL_NAMES,
};

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

#[test]
fn default_builtin_tool_names_match_registry_definitions() {
    let expected = default_builtin_tool_names();
    let definitions = create_all_tool_definitions();

    assert_eq!(expected.len(), BUILTIN_TOOL_NAMES.len());
    for tool_name in expected {
        assert!(
            definitions.contains_key(&tool_name),
            "missing tool definition for {tool_name}"
        );
    }
}
