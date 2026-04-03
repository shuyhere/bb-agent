//! Bordered multi-line editor state and shared types.

use crate::component::Focusable;
use crate::kill_ring::KillRing;
use crate::select_list::{SelectItem, SelectList};
use crate::slash_commands::shared_slash_command_select_items;
use crate::undo_stack::UndoStack;
use std::path::PathBuf;

/// Editor state.
pub(super) struct EditorState {
    pub(super) lines: Vec<String>,
    pub(super) cursor_line: usize,
    pub(super) cursor_col: usize,
    /// Anchor point for text selection (line, col). When set, selection extends from anchor to cursor.
    pub(super) selection_anchor: Option<(usize, usize)>,
}

/// Snapshot for undo/redo.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct EditorSnapshot {
    pub(super) lines: Vec<String>,
    pub(super) cursor_line: usize,
    pub(super) cursor_col: usize,
}

/// Tracks the last editor action for kill-accumulation and yank-pop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LastAction {
    Kill,
    Yank { len: usize },
    Other,
}

/// A multi-line editor component with top/bottom border.
pub struct Editor {
    pub(super) state: EditorState,
    pub(super) focused: bool,
    /// Terminal rows (updated externally for scroll calculation).
    pub terminal_rows: u16,
    /// Vertical scroll offset.
    pub(super) scroll_offset: usize,
    /// History of submitted inputs.
    pub(super) history: Vec<String>,
    /// Current position while browsing history (-1 = live).
    pub(super) history_index: isize,
    /// Callback when user submits.
    pub(super) on_submit: Option<Box<dyn Fn(&str) + Send>>,
    /// Whether submit is disabled.
    pub disable_submit: bool,
    /// Border color escape code (default: dim).
    pub border_color: String,
    /// Slash command autocomplete menu.
    pub(super) slash_menu: Option<SelectList>,
    pub(super) slash_commands: Vec<SelectItem>,
    /// Kill ring for Emacs-style kill/yank.
    pub(super) kill_ring: KillRing,
    /// Undo stack.
    pub(super) undo_stack: UndoStack<EditorSnapshot>,
    /// Redo stack.
    pub(super) redo_stack: UndoStack<EditorSnapshot>,
    /// Last action (for kill accumulation and yank-pop).
    pub(super) last_action: LastAction,
    /// @file autocomplete menu.
    pub(super) file_menu: Option<SelectList>,
    /// Current working directory for file scanning.
    pub(super) cwd: PathBuf,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            state: EditorState {
                lines: vec![String::new()],
                cursor_line: 0,
                cursor_col: 0,
                selection_anchor: None,
            },
            focused: false,
            terminal_rows: 24,
            scroll_offset: 0,
            history: Vec::new(),
            history_index: -1,
            on_submit: None,
            disable_submit: false,
            border_color: "\x1b[38;2;178;148;187m".to_string(), // pi-style purple
            slash_menu: None,
            slash_commands: shared_slash_command_select_items(),
            kill_ring: KillRing::default(),
            undo_stack: UndoStack::default(),
            redo_stack: UndoStack::default(),
            last_action: LastAction::Other,
            file_menu: None,
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }

    pub fn set_on_submit<F: Fn(&str) + Send + 'static>(&mut self, f: F) {
        self.on_submit = Some(Box::new(f));
    }

    /// Get the full text content.
    pub fn get_text(&self) -> String {
        self.state.lines.join("\n")
    }

    /// Set the text content.
    pub fn set_text(&mut self, text: &str) {
        self.state.lines = text.split('\n').map(|s| s.to_string()).collect();
        if self.state.lines.is_empty() {
            self.state.lines.push(String::new());
        }
        self.state.cursor_line = self.state.lines.len() - 1;
        self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
        self.scroll_offset = 0;
        self.update_slash_menu();
        self.update_file_menu();
    }

    /// Clear the editor.
    pub fn clear(&mut self) {
        self.state.lines = vec![String::new()];
        self.state.cursor_line = 0;
        self.state.cursor_col = 0;
        self.state.selection_anchor = None;
        self.scroll_offset = 0;
        self.history_index = -1;
        self.slash_menu = None;
        self.file_menu = None;
    }

    /// Add text to history.
    pub fn add_to_history(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.history.first().map(|s| s.as_str()) == Some(trimmed) {
            return;
        }
        self.history.insert(0, trimmed.to_string());
        if self.history.len() > 100 {
            self.history.pop();
        }
    }

    /// Take the submitted text, clearing the editor and returning the text.
    /// Returns None if no submit callback was triggered.
    pub fn take_submitted(&mut self) -> Option<String> {
        // This is called from the event loop after handle_input
        // We use a separate channel for this
        None
    }

    pub(super) fn is_on_first_visual_line(&self) -> bool {
        self.state.cursor_line == 0
    }

    pub(super) fn is_on_last_visual_line(&self) -> bool {
        self.state.cursor_line == self.state.lines.len() - 1
    }

    pub(super) fn is_empty(&self) -> bool {
        self.state.lines.len() == 1 && self.state.lines[0].is_empty()
    }

    pub fn is_showing_slash_menu(&self) -> bool {
        self.slash_menu.is_some()
    }

    pub fn is_showing_file_menu(&self) -> bool {
        self.file_menu.is_some()
    }

    /// Set the current working directory for file scanning.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        self.cwd = cwd;
    }
}

impl Focusable for Editor {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
