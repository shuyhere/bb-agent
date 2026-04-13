pub(crate) const TOOL_EXPAND_HINT: &str = "Ctrl+Shift+O to expand";
pub(crate) const TOOL_COLLAPSE_HINT: &str = "Ctrl+Shift+O to collapse";
pub(crate) const TUI_TOOL_STATUS_HINT: &str =
    "Ctrl+Shift+O expands tools • Enter submits • Shift+Enter newline • wheel scrolls transcript";
pub(crate) const TUI_HEADER_TOOL_HINT: &str = "Ctrl+Shift+O expands tools";
pub(crate) const CLIPBOARD_EMPTY_HINT: &str = "No clipboard text or image available for paste";
pub(crate) const NO_BLOCK_FOCUSED_HINT: &str = "no block focused";
pub(crate) const TOOL_FAILED_NO_TEXT_OUTPUT: &str = "tool failed with no textual output";
pub(crate) const NO_TEXT_OUTPUT: &str = "(no textual output)";

pub(crate) fn more_lines_expand_hint(hidden: usize) -> String {
    format!("... ({hidden} more lines; {TOOL_EXPAND_HINT})")
}

pub(crate) fn earlier_lines_expand_hint(hidden: usize) -> String {
    format!("... ({hidden} earlier lines; {TOOL_EXPAND_HINT})")
}

pub(crate) fn more_links_expand_hint(hidden: usize) -> String {
    format!("... ({hidden} more link(s); {TOOL_EXPAND_HINT})")
}

pub(crate) fn collapsed_tool_summary_hint(verb: &str, count: usize, noun: &str) -> String {
    format!("{verb} {count} {noun} ({TOOL_EXPAND_HINT})")
}

pub(crate) fn image_placeholder(mime_type: &str) -> String {
    format!("[image: {mime_type}]")
}
