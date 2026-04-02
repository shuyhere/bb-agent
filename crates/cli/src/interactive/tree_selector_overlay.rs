use bb_tui::component::{Component, Focusable};
use bb_tui::utils::visible_width;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::any::Any;

#[derive(Debug, Clone)]
pub enum TreeSelectorAction {
    Selected(String), // entry_id to navigate to
    Cancelled,
    Pending,
}

/// A flattened tree entry for display.
#[derive(Clone)]
pub struct FlatTreeEntry {
    pub entry_id: String,
    pub entry_type: String,    // "user", "assistant", "tool_result", etc.
    pub preview: String,       // first line of message content
    pub timestamp: String,
    pub indent: usize,         // tree depth
    pub is_leaf: bool,         // current leaf?
    pub is_branch_point: bool, // has multiple children?
    pub connector: String,     // tree connector chars
}

pub struct TreeSelectorOverlay {
    entries: Vec<FlatTreeEntry>,
    selected: usize,
    action: TreeSelectorAction,
    focused: bool,
    scroll_offset: usize,
}

impl TreeSelectorOverlay {
    pub fn new(entries: Vec<FlatTreeEntry>, initial_leaf_idx: Option<usize>) -> Self {
        let selected = initial_leaf_idx.unwrap_or(entries.len().saturating_sub(1));
        Self {
            entries,
            selected,
            action: TreeSelectorAction::Pending,
            focused: true,
            scroll_offset: 0,
        }
    }

    pub fn action(&self) -> &TreeSelectorAction {
        &self.action
    }
}

const BORDER_COLOR: &str = "\x1b[38;2;178;148;187m";
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";

fn role_indicator(entry_type: &str) -> (&str, &str) {
    match entry_type {
        "user" => ("U", CYAN),
        "assistant" => ("A", GREEN),
        "tool_result" => ("T", YELLOW),
        "compaction" => ("C", DIM),
        _ => ("?", DIM),
    }
}

impl Component for TreeSelectorOverlay {
    fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();
        let border = format!("{BORDER_COLOR}{}{RESET}", "\u{2500}".repeat(w));

        lines.push(border.clone());
        lines.push(format!("  {BOLD}Session Tree{RESET}  {DIM}(Enter: navigate, Esc: cancel){RESET}"));
        lines.push(String::new());

        if self.entries.is_empty() {
            lines.push(format!("  {DIM}No entries in session{RESET}"));
        } else {
            let max_visible = 20;
            // Keep selected in view
            let start = if self.selected >= self.scroll_offset + max_visible {
                self.selected + 1 - max_visible
            } else if self.selected < self.scroll_offset {
                self.selected
            } else {
                self.scroll_offset
            };

            for i in start..(start + max_visible).min(self.entries.len()) {
                let entry = &self.entries[i];
                let is_selected = i == self.selected;
                let (role_char, role_color) = role_indicator(&entry.entry_type);

                let leaf_marker = if entry.is_leaf {
                    format!(" {GREEN}<--{RESET}")
                } else {
                    String::new()
                };

                let branch_marker = if entry.is_branch_point {
                    format!(" {YELLOW}*{RESET}")
                } else {
                    String::new()
                };

                // Truncate preview to fit
                let prefix_len = 4 + entry.connector.len() + 4; // cursor + connector + role + space
                let suffix_len = if entry.is_leaf { 5 } else { 0 }
                    + if entry.is_branch_point { 3 } else { 0 };
                let available = w.saturating_sub(prefix_len + suffix_len + 2);
                let preview = if entry.preview.chars().count() > available {
                    let truncated: String = entry.preview.chars().take(available.saturating_sub(3)).collect();
                    format!("{truncated}...")
                } else {
                    entry.preview.clone()
                };

                let cursor = if is_selected {
                    format!("{BORDER_COLOR}>{RESET} ")
                } else {
                    "  ".to_string()
                };

                let text_style = if is_selected { BOLD } else { "" };
                let text_reset = if is_selected { RESET } else { "" };

                let line = format!(
                    "  {cursor}{}{role_color}[{role_char}]{RESET} {text_style}{preview}{text_reset}{leaf_marker}{branch_marker}",
                    entry.connector,
                );

                let vis = visible_width(&line);
                let pad = w.saturating_sub(vis);
                lines.push(format!("{line}{}", " ".repeat(pad)));
            }

            if self.entries.len() > max_visible {
                let showing = max_visible.min(self.entries.len());
                lines.push(format!(
                    "  {DIM}{}/{} entries{RESET}",
                    showing, self.entries.len()
                ));
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "{DIM}  Up/Down: navigate  Enter: switch to entry  Esc: cancel{RESET}"
        ));
        lines.push(border);
        lines
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                if self.selected + 1 < self.entries.len() {
                    self.selected += 1;
                }
            }
            (KeyCode::Home, _) => {
                self.selected = 0;
            }
            (KeyCode::End, _) => {
                self.selected = self.entries.len().saturating_sub(1);
            }
            (KeyCode::PageUp, _) => {
                self.selected = self.selected.saturating_sub(10);
            }
            (KeyCode::PageDown, _) => {
                self.selected = (self.selected + 10).min(self.entries.len().saturating_sub(1));
            }
            (KeyCode::Enter, _) => {
                if let Some(entry) = self.entries.get(self.selected) {
                    self.action = TreeSelectorAction::Selected(entry.entry_id.clone());
                }
            }
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.action = TreeSelectorAction::Cancelled;
            }
            _ => {}
        }
    }

    fn invalidate(&mut self) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Focusable for TreeSelectorOverlay {
    fn focused(&self) -> bool {
        self.focused
    }
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}
