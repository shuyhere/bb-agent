mod preview;

use std::collections::HashSet;

use bb_session::tree::TreeNode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::theme::theme;
use crate::utils::{pad_to_width, truncate_to_width};

// ── Flat node for display ────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FlatNode {
    entry_id: String,
    parent_id: Option<String>,
    entry_type: String,
    depth: usize,
    preview: String,
    is_active: bool,
    has_children: bool,
    is_last_child: bool,
    /// For each depth level, whether the ancestor at that level is a last child.
    /// Used to decide whether to draw `│  ` or `   ` for indentation.
    ancestor_is_last: Vec<bool>,
    /// Entry IDs of all ancestors from root → parent.
    ancestor_ids: Vec<String>,
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

#[derive(Clone, Debug)]
pub struct TreeSelector {
    all_nodes: Vec<FlatNode>,
    visible: Vec<usize>,
    selected: usize,
    scroll_offset: usize,
    max_visible: usize,
    filter: TreeFilter,
    folded: HashSet<String>,
}

impl TreeSelector {
    pub fn new(tree: Vec<TreeNode>, active_leaf: Option<&str>, max_visible: usize) -> Self {
        let all_nodes = flatten(&tree, 0, active_leaf, &[], &[]);
        let visible: Vec<usize> = (0..all_nodes.len()).collect();

        // Pre-select the active leaf if present
        let selected = all_nodes.iter().position(|n| n.is_active).unwrap_or(0);

        let mut sel = Self {
            all_nodes,
            visible,
            selected: 0,
            scroll_offset: 0,
            max_visible,
            filter: TreeFilter::All,
            folded: HashSet::new(),
        };
        // Set selected in visible space
        sel.selected = sel.visible.iter().position(|&i| i == selected).unwrap_or(0);
        sel.refilter();
        sel.adjust_scroll();
        sel
    }

    pub fn selected_value(&self) -> Option<String> {
        self.visible
            .get(self.selected)
            .and_then(|idx| self.all_nodes.get(*idx))
            .map(|node| node.entry_id.clone())
    }

    pub fn set_selected_value(&mut self, value: &str) {
        if let Some(pos) = self.visible.iter().position(|&idx| {
            self.all_nodes
                .get(idx)
                .is_some_and(|node| node.entry_id == value)
        }) {
            self.selected = pos;
            self.adjust_scroll();
        }
    }

    pub fn set_max_visible(&mut self, max_visible: usize) {
        self.max_visible = max_visible.max(1);
        self.adjust_scroll();
    }

    /// Render the tree into displayable lines.
    pub fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let t = theme();
        let mut lines = Vec::new();

        let preview_color = |node: &FlatNode| -> &str {
            match node.entry_type.as_str() {
                "message" if node.preview.starts_with("user:") => &t.border,
                "message" if node.preview.starts_with("assistant:") => &t.success,
                _ => &t.muted,
            }
        };

        // Header
        let filter_label = match self.filter {
            TreeFilter::All => "conversation",
            TreeFilter::UserOnly => "user only",
        };
        lines.push(format!(
            "{}{}🌳 Session Tree{}{} ({filter_label})  [Ctrl+U filter · [/] fold · Esc close]{}",
            t.bold, t.accent, t.reset, t.muted, t.reset
        ));
        lines.push(format!(
            "{}{}{}",
            t.border_muted,
            "─".repeat(w.min(60)),
            t.reset
        ));

        if self.visible.is_empty() {
            lines.push(format!("{}  (empty tree){}", t.muted, t.reset));
            return lines;
        }

        let total = self.visible.len();
        let visible_count = total.min(self.max_visible);
        let end = (self.scroll_offset + visible_count).min(total);

        if self.scroll_offset > 0 {
            lines.push(format!(
                "{}  ▲ {} more above{}",
                t.muted, self.scroll_offset, t.reset
            ));
        }

        for vi in self.scroll_offset..end {
            let node = &self.all_nodes[self.visible[vi]];
            let is_selected = vi == self.selected;

            let mut indent = String::new();
            for d in 0..node.depth {
                if d < node.ancestor_is_last.len() && node.ancestor_is_last[d] {
                    indent.push_str("   ");
                } else {
                    indent.push_str("│  ");
                }
            }

            let connector = if node.depth == 0 {
                ""
            } else if node.is_last_child {
                "└"
            } else {
                "├"
            };
            let fold = if node.has_children {
                if self.folded.contains(&node.entry_id) {
                    "⊞"
                } else {
                    "⊟"
                }
            } else if node.depth == 0 {
                "•"
            } else {
                "─"
            };

            let active_plain = if node.is_active { "  ← active" } else { "" };
            let branch = if node.is_active { "• " } else { "  " };

            if is_selected {
                let rest_plain = format!(
                    "▶ {}{}{} {branch}{}{}",
                    indent, connector, fold, node.preview, active_plain
                );
                let rest = pad_to_width(
                    &truncate_to_width(&rest_plain, w.saturating_sub(2)),
                    w.saturating_sub(2),
                );
                lines.push(format!(
                    "{}{}{}▎ {}{}{}",
                    t.selected_bg,
                    t.bold,
                    t.border_accent,
                    preview_color(node),
                    rest,
                    t.reset
                ));
            } else {
                let branch_preview = format!("{branch}{}", node.preview);
                let active_suffix = if node.is_active {
                    format!("  {}{}← active{}", t.bold, t.warning, t.reset)
                } else {
                    String::new()
                };
                let line = format!(
                    " {}{}{}{}{} {}{}{}{}",
                    if node.is_active { "•" } else { " " },
                    t.border_muted,
                    indent,
                    connector,
                    fold,
                    preview_color(node),
                    branch_preview,
                    t.reset,
                    active_suffix,
                );
                lines.push(line);
            }
        }

        if end < total {
            lines.push(format!(
                "{}  ▼ {} more below{}",
                t.muted,
                total - end,
                t.reset
            ));
        }

        lines.push(format!(
            "{} {}/{} nodes · ↑↓ navigate · Enter select · [/] fold · Esc close{}",
            t.muted,
            self.visible.len(),
            self.all_nodes.len(),
            t.reset
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
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.toggle_filter();
                TreeAction::None
            }
            (KeyCode::Char('['), _) => {
                self.fold_or_parent();
                TreeAction::None
            }
            (KeyCode::Char(']'), _) => {
                self.unfold_or_child();
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

    fn fold_or_parent(&mut self) {
        let Some(current_id) = self.selected_value() else {
            return;
        };
        let Some(node) = self
            .all_nodes
            .iter()
            .find(|node| node.entry_id == current_id)
            .cloned()
        else {
            return;
        };
        if node.has_children && !self.folded.contains(&node.entry_id) {
            self.folded.insert(node.entry_id);
            self.refilter();
        } else if let Some(parent_id) = node.parent_id {
            self.set_selected_value(&parent_id);
        }
    }

    fn unfold_or_child(&mut self) {
        let Some(current_id) = self.selected_value() else {
            return;
        };
        let Some(node) = self
            .all_nodes
            .iter()
            .find(|node| node.entry_id == current_id)
            .cloned()
        else {
            return;
        };
        if self.folded.remove(&node.entry_id) {
            self.refilter();
            return;
        }
        if !node.has_children {
            return;
        }
        let child_id = self
            .all_nodes
            .iter()
            .find(|candidate| candidate.parent_id.as_deref() == Some(node.entry_id.as_str()))
            .map(|child| child.entry_id.clone());
        if let Some(child_id) = child_id {
            self.set_selected_value(&child_id);
        }
    }

    fn is_default_visible(_node: &FlatNode) -> bool {
        true
    }

    fn is_user_visible(node: &FlatNode) -> bool {
        node.entry_type == "message" && node.preview.starts_with("user:")
    }

    fn refilter(&mut self) {
        let selected_entry = self.selected_value();
        self.visible = self
            .all_nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                let matches_filter = match self.filter {
                    TreeFilter::All => Self::is_default_visible(node),
                    TreeFilter::UserOnly => Self::is_user_visible(node),
                };
                let hidden_by_fold = node
                    .ancestor_ids
                    .iter()
                    .any(|ancestor| self.folded.contains(ancestor));
                matches_filter && !hidden_by_fold
            })
            .map(|(i, _)| i)
            .collect();

        if let Some(selected_entry) = selected_entry {
            if let Some(pos) = self.visible.iter().position(|&idx| {
                self.all_nodes
                    .get(idx)
                    .is_some_and(|node| node.entry_id == selected_entry)
            }) {
                self.selected = pos;
            } else {
                self.selected = self.selected.min(self.visible.len().saturating_sub(1));
            }
        } else {
            self.selected = self.selected.min(self.visible.len().saturating_sub(1));
        }
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
    ancestor_ids: &[String],
) -> Vec<FlatNode> {
    let mut flat = Vec::new();
    let count = nodes.len();

    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == count - 1;
        let preview = extract_preview(node);
        let is_active = active_leaf.map(|l| l == node.entry_id).unwrap_or(false);

        flat.push(FlatNode {
            entry_id: node.entry_id.clone(),
            parent_id: node.parent_id.clone(),
            entry_type: node.entry_type.clone(),
            depth,
            preview,
            is_active,
            has_children: !node.children.is_empty(),
            is_last_child: is_last,
            ancestor_is_last: ancestor_is_last.to_vec(),
            ancestor_ids: ancestor_ids.to_vec(),
        });

        // Recurse into children with updated ancestor metadata.
        let mut child_ancestors = ancestor_is_last.to_vec();
        child_ancestors.push(is_last);
        let mut child_ancestor_ids = ancestor_ids.to_vec();
        child_ancestor_ids.push(node.entry_id.clone());
        flat.extend(flatten(
            &node.children,
            depth + 1,
            active_leaf,
            &child_ancestors,
            &child_ancestor_ids,
        ));
    }
    flat
}

// ── Extract preview ──────────────────────────────────────────────────

use preview::{build_visible_nodes, extract_preview, flatten_visible};
#[cfg(test)]
use preview::{extract_preview_from_payload, truncate_str};

/// Build a TreeSelector with previews extracted from entry payloads and a
/// display topology that only increases depth on true forks.
pub fn build_tree_selector(
    tree: Vec<TreeNode>,
    entries: &[bb_session::store::EntryRow],
    active_leaf: Option<&str>,
    max_visible: usize,
) -> TreeSelector {
    let payloads: std::collections::HashMap<&str, &str> = entries
        .iter()
        .map(|e| (e.entry_id.as_str(), e.payload.as_str()))
        .collect();

    let visible_tree = build_visible_nodes(&tree, &payloads, active_leaf);
    let all_nodes = flatten_visible(&visible_tree, 0, None, &[], &[]);
    let visible: Vec<usize> = (0..all_nodes.len()).collect();
    let selected_raw = all_nodes.iter().position(|n| n.is_active).unwrap_or(0);

    let mut selector = TreeSelector {
        all_nodes,
        visible,
        selected: 0,
        scroll_offset: 0,
        max_visible,
        filter: TreeFilter::All,
        folded: HashSet::new(),
    };
    selector.selected = selector
        .visible
        .iter()
        .position(|&i| i == selected_raw)
        .unwrap_or(0);
    selector.refilter();
    selector.adjust_scroll();
    selector
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
