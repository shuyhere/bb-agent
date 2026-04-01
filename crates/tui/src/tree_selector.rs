use bb_session::tree::TreeNode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{Attribute, Color, Stylize};

// ── Flat node for display ────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FlatNode {
    entry_id: String,
    entry_type: String,
    depth: usize,
    preview: String,
    is_active: bool,
    has_children: bool,
    is_last_child: bool,
    /// For each depth level, whether the ancestor at that level is a last child.
    /// Used to decide whether to draw `│  ` or `   ` for indentation.
    ancestor_is_last: Vec<bool>,
    #[allow(dead_code)]
    timestamp: String,
}

// ── Filter ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum TreeFilter {
    All,
    UserOnly,
}

// ── Action returned from key handling ────────────────────────────────

pub enum TreeAction {
    None,
    Selected(String),
    Cancelled,
}

// ── TreeSelector ─────────────────────────────────────────────────────

pub struct TreeSelector {
    all_nodes: Vec<FlatNode>,
    visible: Vec<usize>,
    selected: usize,
    scroll_offset: usize,
    max_visible: usize,
    #[allow(dead_code)]
    active_leaf: Option<String>,
    filter: TreeFilter,
}

impl TreeSelector {
    pub fn new(tree: Vec<TreeNode>, active_leaf: Option<&str>, max_visible: usize) -> Self {
        let all_nodes = flatten(&tree, 0, active_leaf, &[]);
        let visible: Vec<usize> = (0..all_nodes.len()).collect();

        // Pre-select the active leaf if present
        let selected = all_nodes
            .iter()
            .position(|n| n.is_active)
            .unwrap_or(0);

        let mut sel = Self {
            all_nodes,
            visible,
            selected: 0,
            scroll_offset: 0,
            max_visible,
            active_leaf: active_leaf.map(|s| s.to_string()),
            filter: TreeFilter::All,
        };
        // Set selected in visible space
        sel.selected = sel.visible.iter().position(|&i| i == selected).unwrap_or(0);
        sel.adjust_scroll();
        sel
    }

    /// Render the tree into displayable lines.
    pub fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();

        // Header
        let filter_label = match self.filter {
            TreeFilter::All => "all",
            TreeFilter::UserOnly => "user only",
        };
        lines.push(format!(
            " {} Session Tree ({})  [Ctrl+U toggle filter]",
            "🌳",
            filter_label,
        ));
        lines.push(format!(
            "{}",
            "─".repeat(w.min(60)).with(Color::DarkGrey)
        ));

        if self.visible.is_empty() {
            lines.push(format!(
                "  {}",
                "(empty tree)".with(Color::DarkGrey).attribute(Attribute::Dim)
            ));
            return lines;
        }

        let total = self.visible.len();
        let visible_count = total.min(self.max_visible);
        let end = (self.scroll_offset + visible_count).min(total);

        // Scroll-up indicator
        if self.scroll_offset > 0 {
            lines.push(format!(
                "  {} {} more above",
                "▲".with(Color::DarkGrey),
                self.scroll_offset.to_string().with(Color::DarkGrey),
            ));
        }

        for vi in self.scroll_offset..end {
            let node = &self.all_nodes[self.visible[vi]];
            let is_selected = vi == self.selected;

            // Build indent
            let mut indent = String::new();
            for d in 0..node.depth {
                if d < node.ancestor_is_last.len() && node.ancestor_is_last[d] {
                    indent.push_str("   ");
                } else {
                    indent.push_str("│  ");
                }
            }

            // Connector
            let connector = if node.depth == 0 && !node.is_last_child {
                "├─"
            } else if node.depth == 0 && node.is_last_child {
                "└─"
            } else if node.is_last_child {
                "└─"
            } else {
                "├─"
            };

            // Type icon and color
            let (type_label, color) = match node.entry_type.as_str() {
                "message" if node.preview.starts_with("user:") => ("", Color::Blue),
                "message" => ("", Color::Green),
                "compaction" => ("", Color::DarkGrey),
                "branch_summary" => ("", Color::DarkGrey),
                "model_change" => ("", Color::Yellow),
                _ => ("", Color::DarkGrey),
            };

            let _ = type_label; // We embed type in the preview already

            // Active marker
            let active_marker = if node.is_active {
                format!("  {}", "← active".with(Color::Yellow).bold())
            } else {
                String::new()
            };

            // Selection marker
            let sel_marker = if is_selected { ">" } else { " " };
            let sel_styled = if is_selected {
                format!("{}", sel_marker.with(Color::Cyan).bold())
            } else {
                sel_marker.to_string()
            };

            let preview_styled = if is_selected {
                format!(
                    "{}",
                    node.preview
                        .clone()
                        .with(color)
                        .bold()
                        .attribute(Attribute::Reverse)
                )
            } else {
                format!("{}", node.preview.clone().with(color))
            };

            let line = format!(
                "{}{}{} {} {}{}",
                sel_styled,
                indent.with(Color::DarkGrey),
                connector.with(Color::DarkGrey),
                preview_styled,
                "",
                active_marker,
            );

            lines.push(line);
        }

        // Scroll-down indicator
        if end < total {
            lines.push(format!(
                "  {} {} more below",
                "▼".with(Color::DarkGrey),
                (total - end).to_string().with(Color::DarkGrey),
            ));
        }

        // Footer
        lines.push(format!(
            "{}",
            format!(
                " {}/{} nodes · ↑↓ navigate · Enter select · Esc cancel",
                self.visible.len(),
                self.all_nodes.len()
            )
            .with(Color::DarkGrey)
            .attribute(Attribute::Dim)
        ));

        lines
    }

    /// Handle a key event and return the resulting action.
    pub fn handle_key(&mut self, key: KeyEvent) -> TreeAction {
        match (key.code, key.modifiers) {
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                self.move_up(1);
                TreeAction::None
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                self.move_down(1);
                TreeAction::None
            }
            (KeyCode::PageUp, _) => {
                self.move_up(self.max_visible);
                TreeAction::None
            }
            (KeyCode::PageDown, _) => {
                self.move_down(self.max_visible);
                TreeAction::None
            }
            (KeyCode::Home, _) => {
                self.selected = 0;
                self.scroll_offset = 0;
                TreeAction::None
            }
            (KeyCode::End, _) => {
                if !self.visible.is_empty() {
                    self.selected = self.visible.len() - 1;
                    self.adjust_scroll();
                }
                TreeAction::None
            }
            (KeyCode::Enter, _) => {
                if let Some(&idx) = self.visible.get(self.selected) {
                    TreeAction::Selected(self.all_nodes[idx].entry_id.clone())
                } else {
                    TreeAction::Cancelled
                }
            }
            (KeyCode::Esc, _) => TreeAction::Cancelled,
            // Ctrl+U: toggle user-only filter
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.toggle_filter();
                TreeAction::None
            }
            _ => TreeAction::None,
        }
    }

    fn toggle_filter(&mut self) {
        self.filter = match self.filter {
            TreeFilter::All => TreeFilter::UserOnly,
            TreeFilter::UserOnly => TreeFilter::All,
        };
        self.refilter();
    }

    fn refilter(&mut self) {
        self.visible = match self.filter {
            TreeFilter::All => (0..self.all_nodes.len()).collect(),
            TreeFilter::UserOnly => self
                .all_nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    n.entry_type == "message" && n.preview.starts_with("user:")
                })
                .map(|(i, _)| i)
                .collect(),
        };
        // Try to keep selection on same node, otherwise reset
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
        self.scroll_offset = 0;
        self.adjust_scroll();
    }

    fn move_up(&mut self, n: usize) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(n);
        self.adjust_scroll();
    }

    fn move_down(&mut self, n: usize) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = (self.selected + n).min(self.visible.len() - 1);
        self.adjust_scroll();
    }

    fn adjust_scroll(&mut self) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + self.max_visible {
            self.scroll_offset = self.selected + 1 - self.max_visible;
        }
    }
}

// ── Flatten tree ─────────────────────────────────────────────────────

fn flatten(
    nodes: &[TreeNode],
    depth: usize,
    active_leaf: Option<&str>,
    ancestor_is_last: &[bool],
) -> Vec<FlatNode> {
    let mut flat = Vec::new();
    let count = nodes.len();

    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == count - 1;
        let preview = extract_preview(node);
        let is_active = active_leaf
            .map(|l| l == node.entry_id)
            .unwrap_or(false);

        flat.push(FlatNode {
            entry_id: node.entry_id.clone(),
            entry_type: node.entry_type.clone(),
            depth,
            preview,
            is_active,
            has_children: !node.children.is_empty(),
            is_last_child: is_last,
            ancestor_is_last: ancestor_is_last.to_vec(),
            timestamp: node.timestamp.clone(),
        });

        // Recurse into children with updated ancestor_is_last
        let mut child_ancestors = ancestor_is_last.to_vec();
        child_ancestors.push(is_last);
        flat.extend(flatten(
            &node.children,
            depth + 1,
            active_leaf,
            &child_ancestors,
        ));
    }
    flat
}

// ── Extract preview ──────────────────────────────────────────────────

fn extract_preview(node: &TreeNode) -> String {
    // The TreeNode doesn't carry payload directly — we use entry_type
    // and the entry_id. For a richer preview we'd need the payload,
    // but TreeNode only has entry_type. We produce a type-based label.
    // In practice, callers can enrich FlatNode with payload data.
    match node.entry_type.as_str() {
        "message" => format!("message: {}", &node.entry_id[..8.min(node.entry_id.len())]),
        "compaction" => "[compaction]".to_string(),
        "branch_summary" => "[branch summary]".to_string(),
        "model_change" => "[model change]".to_string(),
        "thinking_level_change" => "[thinking level change]".to_string(),
        "session_info" => "[session info]".to_string(),
        other => format!("[{other}]"),
    }
}

/// Build a TreeSelector with enriched previews from entry payloads.
/// This version takes the raw entries alongside the tree to extract
/// message previews from the JSON payload.
pub fn build_tree_selector(
    tree: Vec<TreeNode>,
    entries: &[bb_session::store::EntryRow],
    active_leaf: Option<&str>,
    max_visible: usize,
) -> TreeSelector {
    let mut selector = TreeSelector::new(tree, active_leaf, max_visible);

    // Build a lookup from entry_id → payload
    let payloads: std::collections::HashMap<&str, &str> = entries
        .iter()
        .map(|e| (e.entry_id.as_str(), e.payload.as_str()))
        .collect();

    // Enrich previews
    for node in &mut selector.all_nodes {
        if let Some(payload) = payloads.get(node.entry_id.as_str()) {
            node.preview = extract_preview_from_payload(&node.entry_type, payload);
        }
    }

    selector
}

fn extract_preview_from_payload(entry_type: &str, payload: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return format!("[{entry_type}]"),
    };

    match entry_type {
        "message" => {
            // Look for message.role or message content
            if let Some(msg) = parsed.get("message") {
                // User message
                if let Some(content) = msg.get("content") {
                    if let Some(arr) = content.as_array() {
                        for block in arr {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                // Determine role
                                let role = if msg.get("timestamp").is_some()
                                    && msg.get("content").is_some()
                                    && msg.get("provider").is_none()
                                {
                                    "user"
                                } else if msg.get("provider").is_some() {
                                    "assistant"
                                } else {
                                    "message"
                                };

                                let truncated = truncate_str(text, if role == "user" { 60 } else { 40 });
                                return format!("{role}: \"{truncated}\"");
                            }
                        }
                    }
                }
                // Assistant with AssistantContent
                if let Some(content) = msg.get("content") {
                    if let Some(arr) = content.as_array() {
                        for block in arr {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                let truncated = truncate_str(text, 40);
                                return format!("assistant: \"{truncated}\"");
                            }
                        }
                    }
                }
                // ToolResult
                if msg.get("tool_call_id").is_some() {
                    let name = msg
                        .get("tool_name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("tool");
                    return format!("[tool result: {name}]");
                }
            }
            "[message]".to_string()
        }
        "compaction" => {
            let tokens = parsed
                .get("tokens_before")
                .and_then(|t| t.as_u64())
                .map(|t| format!("{t}"))
                .unwrap_or_else(|| "?".to_string());
            format!("[compaction: {tokens} tokens]")
        }
        "branch_summary" => "[branch summary]".to_string(),
        "model_change" => {
            let model = parsed
                .get("model_id")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            format!("[model: {model}]")
        }
        "thinking_level_change" => {
            let level = parsed
                .get("thinking_level")
                .and_then(|l| l.as_str())
                .unwrap_or("?");
            format!("[thinking: {level}]")
        }
        other => format!("[{other}]"),
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    // Take first line, then truncate
    let first_line = s.lines().next().unwrap_or(s);
    let chars: Vec<char> = first_line.chars().collect();
    if chars.len() <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = chars[..max_len].iter().collect();
        format!("{truncated}...")
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bb_session::tree::TreeNode;

    fn make_tree() -> Vec<TreeNode> {
        vec![TreeNode {
            entry_id: "root-1".to_string(),
            parent_id: None,
            entry_type: "message".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            children: vec![
                TreeNode {
                    entry_id: "child-1a".to_string(),
                    parent_id: Some("root-1".to_string()),
                    entry_type: "message".to_string(),
                    timestamp: "2025-01-01T00:01:00Z".to_string(),
                    children: vec![TreeNode {
                        entry_id: "grandchild-1".to_string(),
                        parent_id: Some("child-1a".to_string()),
                        entry_type: "compaction".to_string(),
                        timestamp: "2025-01-01T00:02:00Z".to_string(),
                        children: vec![TreeNode {
                            entry_id: "leaf-active".to_string(),
                            parent_id: Some("grandchild-1".to_string()),
                            entry_type: "message".to_string(),
                            timestamp: "2025-01-01T00:03:00Z".to_string(),
                            children: vec![],
                        }],
                    }],
                },
                TreeNode {
                    entry_id: "child-1b".to_string(),
                    parent_id: Some("root-1".to_string()),
                    entry_type: "message".to_string(),
                    timestamp: "2025-01-01T00:04:00Z".to_string(),
                    children: vec![TreeNode {
                        entry_id: "child-1b-resp".to_string(),
                        parent_id: Some("child-1b".to_string()),
                        entry_type: "message".to_string(),
                        timestamp: "2025-01-01T00:05:00Z".to_string(),
                        children: vec![],
                    }],
                },
            ],
        }]
    }

    #[test]
    fn test_flatten_produces_correct_count() {
        let tree = make_tree();
        let flat = flatten(&tree, 0, Some("leaf-active"), &[]);
        // root + child-1a + grandchild-1 + leaf-active + child-1b + child-1b-resp = 6
        assert_eq!(flat.len(), 6);
    }

    #[test]
    fn test_flatten_depth() {
        let tree = make_tree();
        let flat = flatten(&tree, 0, None, &[]);
        assert_eq!(flat[0].depth, 0); // root-1
        assert_eq!(flat[1].depth, 1); // child-1a
        assert_eq!(flat[2].depth, 2); // grandchild-1
        assert_eq!(flat[3].depth, 3); // leaf-active
        assert_eq!(flat[4].depth, 1); // child-1b
        assert_eq!(flat[5].depth, 2); // child-1b-resp
    }

    #[test]
    fn test_active_leaf_marked() {
        let tree = make_tree();
        let flat = flatten(&tree, 0, Some("leaf-active"), &[]);
        let active_count = flat.iter().filter(|n| n.is_active).count();
        assert_eq!(active_count, 1);
        assert!(flat[3].is_active);
    }

    #[test]
    fn test_selector_creation() {
        let tree = make_tree();
        let selector = TreeSelector::new(tree, Some("leaf-active"), 20);
        assert_eq!(selector.all_nodes.len(), 6);
        assert_eq!(selector.visible.len(), 6);
        // Should pre-select the active leaf
        assert_eq!(selector.selected, 3);
    }

    #[test]
    fn test_selector_navigation() {
        let tree = make_tree();
        let mut selector = TreeSelector::new(tree, None, 20);
        assert_eq!(selector.selected, 0);

        selector.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(selector.selected, 1);

        selector.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(selector.selected, 2);

        selector.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(selector.selected, 1);

        // Home
        selector.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(selector.selected, 5);

        selector.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(selector.selected, 0);
    }

    #[test]
    fn test_selector_enter_returns_selected() {
        let tree = make_tree();
        let mut selector = TreeSelector::new(tree, None, 20);
        selector.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        match selector.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            TreeAction::Selected(id) => assert_eq!(id, "child-1a"),
            _ => panic!("expected Selected"),
        }
    }

    #[test]
    fn test_selector_escape_cancels() {
        let tree = make_tree();
        let mut selector = TreeSelector::new(tree, None, 20);
        assert!(matches!(
            selector.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            TreeAction::Cancelled
        ));
    }

    #[test]
    fn test_render_produces_lines() {
        let tree = make_tree();
        let selector = TreeSelector::new(tree, Some("leaf-active"), 20);
        let lines = selector.render(80);
        assert!(!lines.is_empty());
        // Should contain the active marker somewhere
        let joined = lines.join("\n");
        assert!(joined.contains("active"));
    }

    #[test]
    fn test_render_with_small_width() {
        let tree = make_tree();
        let selector = TreeSelector::new(tree, None, 20);
        let lines = selector.render(30);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_filter_toggle() {
        let tree = make_tree();
        let mut selector = TreeSelector::new(tree, None, 20);
        assert_eq!(selector.filter, TreeFilter::All);
        assert_eq!(selector.visible.len(), 6);

        // Toggle to UserOnly — since our basic preview doesn't start with "user:",
        // all items with entry_type "message" will match the basic "message: ..." preview.
        // Only the compaction node should be filtered out.
        selector.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(selector.filter, TreeFilter::UserOnly);
        // Without enriched previews, none start with "user:" so 0 visible
        assert_eq!(selector.visible.len(), 0);

        // Toggle back
        selector.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(selector.filter, TreeFilter::All);
        assert_eq!(selector.visible.len(), 6);
    }

    #[test]
    fn test_scroll_with_small_visible() {
        let tree = make_tree();
        let mut selector = TreeSelector::new(tree, None, 3);
        // Navigate to end
        for _ in 0..5 {
            selector.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        assert_eq!(selector.selected, 5);
        assert!(selector.scroll_offset > 0);

        let lines = selector.render(80);
        let joined = lines.join("\n");
        assert!(joined.contains("more above"));
    }

    #[test]
    fn test_is_last_child() {
        let tree = make_tree();
        let flat = flatten(&tree, 0, None, &[]);
        // root-1 is last (only) root
        assert!(flat[0].is_last_child);
        // child-1a is NOT last child (child-1b comes after)
        assert!(!flat[1].is_last_child);
        // child-1b IS last child
        assert!(flat[4].is_last_child);
    }

    #[test]
    fn test_extract_preview_from_payload_user() {
        let payload = r#"{"id":"x","timestamp":"2025-01-01T00:00:00Z","message":{"content":[{"text":"Hello, can you help me with something?"}],"timestamp":1234}}"#;
        let preview = extract_preview_from_payload("message", payload);
        assert!(preview.starts_with("user:"));
        assert!(preview.contains("Hello"));
    }

    #[test]
    fn test_extract_preview_from_payload_assistant() {
        let payload = r#"{"id":"x","timestamp":"2025-01-01T00:00:00Z","message":{"content":[{"text":"Sure, I can help you with that!"}],"provider":"anthropic","model":"claude","usage":{"input":0,"output":0},"stop_reason":"stop","timestamp":1234}}"#;
        let preview = extract_preview_from_payload("message", payload);
        assert!(preview.starts_with("assistant:"));
    }

    #[test]
    fn test_extract_preview_from_payload_compaction() {
        let payload = r#"{"id":"x","timestamp":"2025-01-01T00:00:00Z","summary":"...","first_kept_entry_id":"y","tokens_before":12000}"#;
        let preview = extract_preview_from_payload("compaction", payload);
        assert_eq!(preview, "[compaction: 12000 tokens]");
    }

    #[test]
    fn test_extract_preview_from_payload_model_change() {
        let payload = r#"{"id":"x","timestamp":"2025-01-01T00:00:00Z","provider":"anthropic","model_id":"claude-sonnet-4-20250514"}"#;
        let preview = extract_preview_from_payload("model_change", payload);
        assert_eq!(preview, "[model: claude-sonnet-4-20250514]");
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world test", 5), "hello...");
        assert_eq!(truncate_str("line1\nline2", 20), "line1");
    }

    #[test]
    fn test_empty_tree() {
        let selector = TreeSelector::new(vec![], None, 20);
        assert_eq!(selector.all_nodes.len(), 0);
        assert_eq!(selector.visible.len(), 0);
        let lines = selector.render(80);
        assert!(!lines.is_empty()); // header + empty message
    }

    #[test]
    fn test_page_navigation() {
        let tree = make_tree();
        let mut selector = TreeSelector::new(tree, None, 3);
        selector.handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(selector.selected, 3);
        selector.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
        assert_eq!(selector.selected, 0);
    }
}
