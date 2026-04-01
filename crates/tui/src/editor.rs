//! Bordered multi-line editor — matches pi-tui's editor.ts.
//!
//! Features:
//! - Multi-line editing with word wrap
//! - Cursor movement (arrows, Home/End, Ctrl+A/E, word-jump)
//! - Backspace, Delete, Ctrl+K, Ctrl+U
//! - History (Up/Down on first/last line)
//! - Submit on Enter, newline on Alt+Enter / Shift+Enter
//! - Bordered box with horizontal lines (not > prompt)
//! - CURSOR_MARKER emission for hardware cursor positioning
//! - Scrolling within editor area (30% of terminal height)

use crate::component::{Component, Focusable, CURSOR_MARKER};
use crate::fuzzy::fuzzy_filter;
use crate::kill_ring::KillRing;
use crate::select_list::{SelectAction, SelectItem, SelectList};
use crate::undo_stack::UndoStack;
use crate::utils::visible_width;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

/// Simple base64 encoder for OSC 52 clipboard support.
fn base64_encode(input: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut result = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Editor state.
struct EditorState {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
    /// Anchor point for text selection (line, col). When set, selection extends from anchor to cursor.
    selection_anchor: Option<(usize, usize)>,
}

/// Snapshot for undo/redo.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct EditorSnapshot {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
}

/// Tracks the last editor action for kill-accumulation and yank-pop.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LastAction {
    Kill,
    Yank { len: usize },
    Other,
}

/// A multi-line editor component with top/bottom border.
pub struct Editor {
    state: EditorState,
    focused: bool,
    /// Terminal rows (updated externally for scroll calculation).
    pub terminal_rows: u16,
    /// Vertical scroll offset.
    scroll_offset: usize,
    /// History of submitted inputs.
    history: Vec<String>,
    /// Current position while browsing history (-1 = live).
    history_index: isize,
    /// Callback when user submits.
    on_submit: Option<Box<dyn Fn(&str) + Send>>,
    /// Whether submit is disabled.
    pub disable_submit: bool,
    /// Border color escape code (default: dim).
    pub border_color: String,
    /// Slash command autocomplete menu.
    slash_menu: Option<SelectList>,
    slash_commands: Vec<SelectItem>,
    /// Kill ring for Emacs-style kill/yank.
    kill_ring: KillRing,
    /// Undo stack.
    undo_stack: UndoStack<EditorSnapshot>,
    /// Redo stack.
    redo_stack: UndoStack<EditorSnapshot>,
    /// Last action (for kill accumulation and yank-pop).
    last_action: LastAction,
    /// @file autocomplete menu.
    file_menu: Option<SelectList>,
    /// Current working directory for file scanning.
    cwd: PathBuf,
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
            border_color: "\x1b[90m".to_string(), // dim gray
            slash_menu: None,
            slash_commands: default_slash_commands(),
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

    // ── Selection helpers ──

    fn has_selection(&self) -> bool {
        if let Some((al, ac)) = self.state.selection_anchor {
            al != self.state.cursor_line || ac != self.state.cursor_col
        } else {
            false
        }
    }

    /// Returns ordered (start, end) positions of the selection.
    fn selection_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let (al, ac) = self.state.selection_anchor?;
        let (cl, cc) = (self.state.cursor_line, self.state.cursor_col);
        if al == cl && ac == cc {
            return None;
        }
        let start = if al < cl || (al == cl && ac < cc) { (al, ac) } else { (cl, cc) };
        let end = if al < cl || (al == cl && ac < cc) { (cl, cc) } else { (al, ac) };
        Some((start, end))
    }

    /// Get the selected text.
    fn selected_text(&self) -> Option<String> {
        let ((sl, sc), (el, ec)) = self.selection_range()?;
        if sl == el {
            return Some(self.state.lines[sl][sc..ec].to_string());
        }
        let mut result = String::new();
        result.push_str(&self.state.lines[sl][sc..]);
        for i in (sl + 1)..el {
            result.push('\n');
            result.push_str(&self.state.lines[i]);
        }
        result.push('\n');
        result.push_str(&self.state.lines[el][..ec]);
        Some(result)
    }

    /// Delete the selected text and collapse cursor to start of selection.
    fn delete_selection(&mut self) {
        let Some(((sl, sc), (el, ec))) = self.selection_range() else { return };
        if sl == el {
            let line = &self.state.lines[sl];
            let new_line = format!("{}{}", &line[..sc], &line[ec..]);
            self.state.lines[sl] = new_line;
        } else {
            let before = self.state.lines[sl][..sc].to_string();
            let after = self.state.lines[el][ec..].to_string();
            self.state.lines[sl] = format!("{}{}", before, after);
            // Remove lines sl+1..=el
            for _ in (sl + 1)..=el {
                self.state.lines.remove(sl + 1);
            }
        }
        self.state.cursor_line = sl;
        self.state.cursor_col = sc;
        self.state.selection_anchor = None;
    }

    fn clear_selection(&mut self) {
        self.state.selection_anchor = None;
    }

    /// Select all text in the editor.
    fn select_all(&mut self) {
        self.state.selection_anchor = Some((0, 0));
        let last = self.state.lines.len() - 1;
        self.state.cursor_line = last;
        self.state.cursor_col = self.state.lines[last].len();
    }

    /// Set the anchor if starting a new selection, or keep existing anchor.
    fn ensure_anchor(&mut self) {
        if self.state.selection_anchor.is_none() {
            self.state.selection_anchor = Some((self.state.cursor_line, self.state.cursor_col));
        }
    }

    fn is_on_first_visual_line(&self) -> bool {
        self.state.cursor_line == 0
    }

    fn is_on_last_visual_line(&self) -> bool {
        self.state.cursor_line == self.state.lines.len() - 1
    }

    fn is_empty(&self) -> bool {
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

    fn slash_query(&self) -> Option<String> {
        if self.state.cursor_line != 0 {
            return None;
        }
        let line = &self.state.lines[0];
        let before = &line[..self.state.cursor_col.min(line.len())];
        if !before.starts_with('/') {
            return None;
        }
        if before.contains(' ') || before.contains('\n') {
            return None;
        }
        Some(before.to_string())
    }

    fn update_slash_menu(&mut self) {
        let Some(query) = self.slash_query() else {
            self.slash_menu = None;
            return;
        };
        let mut list = SelectList::new(self.slash_commands.clone(), 6);
        list.set_show_search(false);
        let search = query.trim_start_matches('/');
        list.set_search(search);
        self.slash_menu = Some(list);
    }

    fn accept_slash_selection(&mut self, value: String) {
        self.state.lines[0] = value;
        self.state.cursor_line = 0;
        self.state.cursor_col = self.state.lines[0].len();
        self.slash_menu = None;
    }

    /// Detect `@query` before the cursor. Returns the full `@...` token if found.
    fn file_query(&self) -> Option<String> {
        let line = &self.state.lines[self.state.cursor_line];
        let before = &line[..self.state.cursor_col.min(line.len())];
        // Walk backwards to find the `@`
        let mut at_pos = None;
        for (i, c) in before.char_indices().rev() {
            if c == '@' {
                // Make sure it's at start of line or preceded by whitespace
                if i == 0 || before[..i].ends_with(|ch: char| ch.is_whitespace()) {
                    at_pos = Some(i);
                }
                break;
            }
            // If we hit whitespace before finding @, no match
            if c.is_whitespace() {
                return None;
            }
        }
        at_pos.map(|pos| before[pos..].to_string())
    }

    /// Recursively scan a directory for files up to max_depth.
    fn scan_files(dir: &std::path::Path, base: &std::path::Path, depth: usize, max_depth: usize, results: &mut Vec<String>) {
        if depth > max_depth {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip hidden files/dirs
            if name.starts_with('.') {
                continue;
            }
            // Skip common noisy directories
            if path.is_dir() && matches!(name.as_str(), "node_modules" | "target" | "dist" | "build" | ".git" | "__pycache__") {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(base) {
                let rel_str = rel.to_string_lossy().to_string();
                results.push(rel_str);
            }
            if path.is_dir() {
                Self::scan_files(&path, base, depth + 1, max_depth, results);
            }
        }
    }

    fn update_file_menu(&mut self) {
        let Some(query) = self.file_query() else {
            self.file_menu = None;
            return;
        };
        let search = query.trim_start_matches('@');
        // Scan files
        let mut files = Vec::new();
        Self::scan_files(&self.cwd, &self.cwd, 0, 3, &mut files);
        files.sort();

        // Filter using fuzzy_filter
        let filtered = fuzzy_filter(files, search, |f| f.as_str());

        // Build SelectItems from filtered results (cap at 100)
        let items: Vec<SelectItem> = filtered
            .into_iter()
            .take(100)
            .map(|f| SelectItem {
                label: f.clone(),
                detail: None,
                value: f,
            })
            .collect();

        if items.is_empty() {
            self.file_menu = None;
            return;
        }

        let mut list = SelectList::new(items, 8);
        list.set_show_search(false);
        self.file_menu = Some(list);
    }

    fn accept_file_selection(&mut self, path: String) {
        let Some(query) = self.file_query() else {
            self.file_menu = None;
            return;
        };
        let line = &self.state.lines[self.state.cursor_line];
        let before = &line[..self.state.cursor_col.min(line.len())];
        // Find the start of the @query token
        let at_start = before.len() - query.len();
        let replacement = format!("@{}", path);
        let new_line = format!(
            "{}{}{}",
            &line[..at_start],
            replacement,
            &line[self.state.cursor_col..]
        );
        self.state.lines[self.state.cursor_line] = new_line;
        self.state.cursor_col = at_start + replacement.len();
        self.file_menu = None;
    }

    /// Render a line with both selection highlighting and cursor marker.
    fn render_line_with_selection_and_cursor(
        text: &str, cursor_pos: usize, hl_start: usize, hl_end: usize, marker: &str,
    ) -> String {
        // Split the text into regions: before-sel, sel, after-sel
        // The cursor is at cursor_pos bytes into text
        let result;

        // We have up to 5 segments depending on cursor position relative to selection
        let cp = cursor_pos.min(text.len());

        if cp < hl_start {
            // cursor before selection
            let before_cursor = &text[..cp];
            let cursor_to_hl = &text[cp..hl_start];
            let sel = &text[hl_start..hl_end];
            let after_hl = &text[hl_end..];
            let cursor_char: String = cursor_to_hl.chars().next().map(|c| c.to_string()).unwrap_or_default();
            let rest_before = &cursor_to_hl[cursor_char.len()..];
            result = format!(
                "{}{}\x1b[7m{}\x1b[0m{}\x1b[7m{}\x1b[0m{}",
                before_cursor, marker, cursor_char, rest_before, sel, after_hl
            );
        } else if cp >= hl_end {
            // cursor after selection
            let before_hl = &text[..hl_start];
            let sel = &text[hl_start..hl_end];
            let hl_to_cursor = &text[hl_end..cp];
            let after_cursor = &text[cp..];
            if !after_cursor.is_empty() {
                let cursor_char: String = after_cursor.chars().next().map(|c| c.to_string()).unwrap_or_default();
                let rest = &after_cursor[cursor_char.len()..];
                result = format!(
                    "{}\x1b[7m{}\x1b[0m{}{}\x1b[7m{}\x1b[0m{}",
                    before_hl, sel, hl_to_cursor, marker, cursor_char, rest
                );
            } else {
                result = format!(
                    "{}\x1b[7m{}\x1b[0m{}{}\x1b[7m \x1b[0m",
                    before_hl, sel, hl_to_cursor, marker
                );
            }
        } else {
            // cursor inside selection
            let before_hl = &text[..hl_start];
            let sel_before_cursor = &text[hl_start..cp];
            let sel_after_cursor = &text[cp..hl_end];
            let after_hl = &text[hl_end..];
            // Show entire selection highlighted, cursor marker inside
            result = format!(
                "{}\x1b[7m{}\x1b[0m{}\x1b[7m{}\x1b[0m{}",
                before_hl, sel_before_cursor, marker, sel_after_cursor, after_hl
            );
        }

        result
    }

    /// Word-wrap a line into chunks for display.
    fn word_wrap_line(line: &str, max_width: usize) -> Vec<(String, usize, usize)> {
        if max_width == 0 {
            return vec![(String::new(), 0, 0)];
        }
        if line.is_empty() {
            return vec![(String::new(), 0, 0)];
        }

        let line_width = visible_width(line);
        if line_width <= max_width {
            return vec![(line.to_string(), 0, line.len())];
        }

        let mut chunks = Vec::new();
        let chars: Vec<char> = line.chars().collect();
        let mut pos = 0;

        while pos < chars.len() {
            let mut end = pos;
            let mut width = 0;
            let mut last_space = None;

            while end < chars.len() {
                let cw = unicode_width::UnicodeWidthChar::width(chars[end]).unwrap_or(0);
                if width + cw > max_width && end > pos {
                    break;
                }
                if chars[end] == ' ' {
                    last_space = Some(end);
                }
                width += cw;
                end += 1;
            }

            // Try to break at word boundary
            if end < chars.len() && last_space.is_some() && last_space.unwrap() > pos {
                end = last_space.unwrap() + 1;
            }

            let start_byte = chars[..pos].iter().collect::<String>().len();
            let end_byte = chars[..end].iter().collect::<String>().len();
            let text: String = chars[pos..end].iter().collect();
            chunks.push((text, start_byte, end_byte));
            pos = end;
        }

        if chunks.is_empty() {
            chunks.push((String::new(), 0, 0));
        }

        chunks
    }

    /// Build layout lines for rendering.
    fn layout_text(&self, content_width: usize) -> Vec<LayoutLine> {
        let mut layout = Vec::new();

        if self.state.lines.len() == 1 && self.state.lines[0].is_empty() {
            layout.push(LayoutLine {
                text: String::new(),
                has_cursor: true,
                cursor_pos: Some(0),
                line_index: 0,
                byte_start: 0,
                byte_end: 0,
            });
            return layout;
        }

        for (i, line) in self.state.lines.iter().enumerate() {
            let is_current = i == self.state.cursor_line;
            let chunks = Self::word_wrap_line(line, content_width);

            for (ci, (text, start_byte, end_byte)) in chunks.iter().enumerate() {
                let is_last_chunk = ci == chunks.len() - 1;

                if is_current {
                    let col = self.state.cursor_col;
                    let in_chunk = if is_last_chunk {
                        col >= *start_byte
                    } else {
                        col >= *start_byte && col < *end_byte
                    };

                    if in_chunk {
                        let adjusted = col - start_byte;
                        layout.push(LayoutLine {
                            text: text.clone(),
                            has_cursor: true,
                            cursor_pos: Some(adjusted.min(text.len())),
                            line_index: i,
                            byte_start: *start_byte,
                            byte_end: *end_byte,
                        });
                    } else {
                        layout.push(LayoutLine {
                            text: text.clone(),
                            has_cursor: false,
                            cursor_pos: None,
                            line_index: i,
                            byte_start: *start_byte,
                            byte_end: *end_byte,
                        });
                    }
                } else {
                    layout.push(LayoutLine {
                        text: text.clone(),
                        has_cursor: false,
                        cursor_pos: None,
                        line_index: i,
                        byte_start: *start_byte,
                        byte_end: *end_byte,
                    });
                }
            }
        }

        layout
    }

    // ── Input handling ──

    fn insert_char(&mut self, c: char) {
        self.history_index = -1;
        if self.has_selection() {
            self.delete_selection();
        }
        let line = &mut self.state.lines[self.state.cursor_line];
        line.insert(self.state.cursor_col, c);
        self.state.cursor_col += c.len_utf8();
    }

    fn insert_str(&mut self, s: &str) {
        self.history_index = -1;
        if self.has_selection() {
            self.delete_selection();
        }
        for c in s.chars() {
            let line = &mut self.state.lines[self.state.cursor_line];
            line.insert(self.state.cursor_col, c);
            self.state.cursor_col += c.len_utf8();
        }
    }

    fn backspace(&mut self) {
        self.history_index = -1;
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        if self.state.cursor_col > 0 {
            let line = &self.state.lines[self.state.cursor_line];
            // Find the char that ends at cursor_col
            let before = &line[..self.state.cursor_col];
            if let Some(c) = before.chars().last() {
                let char_len = c.len_utf8();
                let new_col = self.state.cursor_col - char_len;
                // Remove the character by rebuilding the string
                let new_line = format!(
                    "{}{}",
                    &self.state.lines[self.state.cursor_line][..new_col],
                    &self.state.lines[self.state.cursor_line][self.state.cursor_col..]
                );
                self.state.lines[self.state.cursor_line] = new_line;
                self.state.cursor_col = new_col;
            }
        } else if self.state.cursor_line > 0 {
            // Merge with previous line
            let current = self.state.lines.remove(self.state.cursor_line);
            self.state.cursor_line -= 1;
            self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
            self.state.lines[self.state.cursor_line].push_str(&current);
        }
    }

    fn delete(&mut self) {
        if self.has_selection() {
            self.delete_selection();
            return;
        }
        let line = &self.state.lines[self.state.cursor_line];
        if self.state.cursor_col < line.len() {
            let chars: Vec<char> = line.chars().collect();
            let mut byte_pos = 0;
            let mut char_idx = 0;
            for (i, c) in chars.iter().enumerate() {
                if byte_pos >= self.state.cursor_col {
                    char_idx = i;
                    break;
                }
                byte_pos += c.len_utf8();
                char_idx = i + 1;
            }
            if char_idx < chars.len() {
                let new_chars: String = chars[..char_idx]
                    .iter()
                    .chain(chars[char_idx + 1..].iter())
                    .collect();
                self.state.lines[self.state.cursor_line] = new_chars;
            }
        } else if self.state.cursor_line + 1 < self.state.lines.len() {
            let next = self.state.lines.remove(self.state.cursor_line + 1);
            self.state.lines[self.state.cursor_line].push_str(&next);
        }
    }

    fn new_line(&mut self) {
        self.history_index = -1;
        if self.has_selection() {
            self.delete_selection();
        }
        let line = &self.state.lines[self.state.cursor_line];
        let rest = line[self.state.cursor_col..].to_string();
        self.state.lines[self.state.cursor_line] = line[..self.state.cursor_col].to_string();
        self.state.cursor_line += 1;
        self.state.lines.insert(self.state.cursor_line, rest);
        self.state.cursor_col = 0;
    }

    fn move_left(&mut self) {
        if self.state.cursor_col > 0 {
            let line = &self.state.lines[self.state.cursor_line];
            let before = &line[..self.state.cursor_col];
            if let Some(c) = before.chars().last() {
                self.state.cursor_col -= c.len_utf8();
            }
        } else if self.state.cursor_line > 0 {
            self.state.cursor_line -= 1;
            self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
        }
    }

    fn move_right(&mut self) {
        let line = &self.state.lines[self.state.cursor_line];
        if self.state.cursor_col < line.len() {
            let after = &line[self.state.cursor_col..];
            if let Some(c) = after.chars().next() {
                self.state.cursor_col += c.len_utf8();
            }
        } else if self.state.cursor_line + 1 < self.state.lines.len() {
            self.state.cursor_line += 1;
            self.state.cursor_col = 0;
        }
    }

    fn move_up(&mut self) {
        if self.state.cursor_line > 0 {
            self.state.cursor_line -= 1;
            self.state.cursor_col = self.state.cursor_col.min(
                self.state.lines[self.state.cursor_line].len(),
            );
        }
    }

    fn move_down(&mut self) {
        if self.state.cursor_line + 1 < self.state.lines.len() {
            self.state.cursor_line += 1;
            self.state.cursor_col = self.state.cursor_col.min(
                self.state.lines[self.state.cursor_line].len(),
            );
        }
    }

    fn move_to_line_start(&mut self) {
        self.state.cursor_col = 0;
    }

    fn move_to_line_end(&mut self) {
        self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
    }

    fn kill_to_end(&mut self, accumulate: bool) {
        let line = &self.state.lines[self.state.cursor_line];
        let killed = line[self.state.cursor_col..].to_string();
        if !killed.is_empty() {
            self.kill_ring.push(&killed, false, accumulate);
        }
        self.state.lines[self.state.cursor_line].truncate(self.state.cursor_col);
    }

    fn kill_to_start(&mut self, accumulate: bool) {
        let line = &self.state.lines[self.state.cursor_line];
        let killed = line[..self.state.cursor_col].to_string();
        if !killed.is_empty() {
            self.kill_ring.push(&killed, true, accumulate);
        }
        let rest = line[self.state.cursor_col..].to_string();
        self.state.lines[self.state.cursor_line] = rest;
        self.state.cursor_col = 0;
    }

    fn word_left(&mut self) {
        if self.state.cursor_col == 0 {
            if self.state.cursor_line > 0 {
                self.state.cursor_line -= 1;
                self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
            }
            return;
        }
        let line = &self.state.lines[self.state.cursor_line];
        let before = &line[..self.state.cursor_col];
        let chars: Vec<char> = before.chars().collect();
        let mut i = chars.len();
        // Skip trailing whitespace
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        // Skip word chars
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        self.state.cursor_col = chars[..i].iter().map(|c| c.len_utf8()).sum();
    }

    fn word_right(&mut self) {
        let line = &self.state.lines[self.state.cursor_line];
        if self.state.cursor_col >= line.len() {
            if self.state.cursor_line + 1 < self.state.lines.len() {
                self.state.cursor_line += 1;
                self.state.cursor_col = 0;
            }
            return;
        }
        let after = &line[self.state.cursor_col..];
        let chars: Vec<char> = after.chars().collect();
        let mut i = 0;
        // Skip word chars
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        // Skip whitespace
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let advance: usize = chars[..i].iter().map(|c| c.len_utf8()).sum();
        self.state.cursor_col += advance;
    }

    fn delete_word_backward(&mut self, accumulate: bool) {
        if self.state.cursor_col == 0 {
            return;
        }
        let line = &self.state.lines[self.state.cursor_line];
        let before = &line[..self.state.cursor_col];
        let chars: Vec<char> = before.chars().collect();
        let mut i = chars.len();
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        let new_col: usize = chars[..i].iter().map(|c| c.len_utf8()).sum();
        let killed = line[new_col..self.state.cursor_col].to_string();
        if !killed.is_empty() {
            self.kill_ring.push(&killed, true, accumulate);
        }
        let rest = &line[self.state.cursor_col..];
        let new_line = format!("{}{}", &line[..new_col], rest);
        self.state.lines[self.state.cursor_line] = new_line;
        self.state.cursor_col = new_col;
    }

    fn navigate_history(&mut self, direction: isize) {
        if self.history.is_empty() {
            return;
        }
        let new_index = self.history_index - direction;
        if new_index < -1 || new_index >= self.history.len() as isize {
            return;
        }
        self.history_index = new_index;
        if self.history_index == -1 {
            self.clear();
        } else {
            let text = self.history[self.history_index as usize].clone();
            self.set_text(&text);
        }
    }

    // ── Snapshot / undo / redo helpers ──

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            lines: self.state.lines.clone(),
            cursor_line: self.state.cursor_line,
            cursor_col: self.state.cursor_col,
        }
    }

    fn restore(&mut self, snap: EditorSnapshot) {
        self.state.lines = snap.lines;
        self.state.cursor_line = snap.cursor_line;
        self.state.cursor_col = snap.cursor_col;
    }

    fn push_undo(&mut self) {
        let snap = self.snapshot();
        self.undo_stack.push(&snap);
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if let Some(snap) = self.undo_stack.pop() {
            let current = self.snapshot();
            self.redo_stack.push(&current);
            self.restore(snap);
        }
    }

    fn redo(&mut self) {
        if let Some(snap) = self.redo_stack.pop() {
            let current = self.snapshot();
            self.undo_stack.push(&current);
            self.restore(snap);
        }
    }

    // ── Yank ──

    fn yank(&mut self) {
        if let Some(text) = self.kill_ring.peek().map(|s| s.to_string()) {
            self.push_undo();
            let len = text.len();
            let line = &mut self.state.lines[self.state.cursor_line];
            line.insert_str(self.state.cursor_col, &text);
            self.state.cursor_col += len;
            self.last_action = LastAction::Yank { len };
        }
    }

    fn yank_pop(&mut self) {
        if let LastAction::Yank { len } = self.last_action {
            if self.kill_ring.len() <= 1 {
                return;
            }
            // Remove previously yanked text
            let start = self.state.cursor_col.saturating_sub(len);
            let line = &self.state.lines[self.state.cursor_line];
            let new_line = format!("{}{}", &line[..start], &line[self.state.cursor_col..]);
            self.state.lines[self.state.cursor_line] = new_line;
            self.state.cursor_col = start;

            // Rotate and insert next entry
            self.kill_ring.rotate();
            if let Some(text) = self.kill_ring.peek().map(|s| s.to_string()) {
                let new_len = text.len();
                let line = &mut self.state.lines[self.state.cursor_line];
                line.insert_str(self.state.cursor_col, &text);
                self.state.cursor_col += new_len;
                self.last_action = LastAction::Yank { len: new_len };
            }
        }
    }

    fn submit(&mut self) -> Option<String> {
        let text = self.get_text().trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.clear();
        self.history_index = -1;
        Some(text)
    }
}

struct LayoutLine {
    text: String,
    has_cursor: bool,
    cursor_pos: Option<usize>,
    /// Which logical line index this layout line comes from.
    line_index: usize,
    /// Byte offset in the original line where this chunk starts.
    byte_start: usize,
    /// Byte offset in the original line where this chunk ends.
    #[allow(dead_code)]
    byte_end: usize,
}

fn default_slash_commands() -> Vec<SelectItem> {
    vec![
        ("help", "Show help"),
        ("new", "Start a new session"),
        ("resume", "Resume a previous session"),
        ("model", "Switch model"),
        ("compact", "Compact conversation context"),
        ("tree", "Navigate session tree"),
        ("fork", "Fork current session"),
        ("name", "Set session display name"),
        ("session", "Show current session info"),
        ("login", "Login to a provider"),
        ("logout", "Logout from a provider"),
        ("settings", "Show settings info"),
        ("quit", "Exit"),
    ]
    .into_iter()
    .map(|(label, detail)| SelectItem {
        label: format!("/{label}"),
        detail: Some(detail.to_string()),
        value: format!("/{label}"),
    })
    .collect()
}

impl Component for Editor {
    crate::impl_as_any!();

    fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        if w < 3 {
            return vec!["─".repeat(w)];
        }

        let content_width = w.saturating_sub(1); // reserve 1 for cursor at end
        let layout_lines = self.layout_text(content_width);

        // Calculate max visible lines: 30% of terminal height, minimum 5
        let max_visible = (self.terminal_rows as usize * 30 / 100).max(5);

        // Find cursor line in layout
        let cursor_line_idx = layout_lines
            .iter()
            .position(|l| l.has_cursor)
            .unwrap_or(0);

        // Adjust scroll
        let mut scroll = self.scroll_offset;
        if cursor_line_idx < scroll {
            scroll = cursor_line_idx;
        } else if cursor_line_idx >= scroll + max_visible {
            scroll = cursor_line_idx - max_visible + 1;
        }
        let max_scroll = layout_lines.len().saturating_sub(max_visible);
        scroll = scroll.min(max_scroll);

        let visible = &layout_lines[scroll..layout_lines.len().min(scroll + max_visible)];

        let border = format!("{}{}{}", self.border_color, "─".repeat(w), "\x1b[0m");
        let mut result = Vec::new();

        // Top border (with scroll indicator if scrolled)
        if scroll > 0 {
            let indicator = format!("─── ↑ {} more ", scroll);
            let remaining = w.saturating_sub(visible_width(&indicator));
            result.push(format!(
                "{}{}{}{}",
                self.border_color,
                indicator,
                "─".repeat(remaining),
                "\x1b[0m"
            ));
        } else {
            result.push(border.clone());
        }

        // Compute selection range for highlighting
        let sel_range = self.selection_range();

        // Content lines
        let emit_cursor = self.focused;
        for ll in visible {
            let mut display = ll.text.clone();

            // Apply selection highlighting
            if let Some(((sl, sc), (el, ec))) = sel_range {
                let li = ll.line_index;
                // Check if this layout line overlaps the selection
                if li >= sl && li <= el {
                    // Compute selection byte range within this chunk
                    let chunk_start = ll.byte_start;
                        let sel_start_in_line = if li == sl { sc } else { 0 };
                    let sel_end_in_line = if li == el { ec } else { self.state.lines[li].len() };

                    // Clamp to this chunk
                    let hl_start = sel_start_in_line.max(chunk_start).saturating_sub(chunk_start);
                    let hl_end = sel_end_in_line.min(chunk_start + ll.text.len()).saturating_sub(chunk_start);

                    if hl_start < hl_end && hl_end <= ll.text.len() {
                        let before_sel = &ll.text[..hl_start];
                        let sel_part = &ll.text[hl_start..hl_end];
                        let after_sel = &ll.text[hl_end..];
                        display = format!(
                            "{}\x1b[7m{}\x1b[0m{}",
                            before_sel, sel_part, after_sel
                        );
                    }
                }
            }

            if ll.has_cursor && emit_cursor {
                if let Some(pos) = ll.cursor_pos {
                    // Re-render with cursor marker (on top of any selection)
                    // We need to work from the raw text for cursor positioning
                    let raw_before = &ll.text[..pos.min(ll.text.len())];
                    let raw_after = &ll.text[pos.min(ll.text.len())..];

                    let marker = CURSOR_MARKER;

                    // Build display with both selection highlight and cursor
                    if let Some(((sl, sc), (el, ec))) = sel_range {
                        let li = ll.line_index;
                        if li >= sl && li <= el {
                            let chunk_start = ll.byte_start;
                            let sel_start_in_line = if li == sl { sc } else { 0 };
                            let sel_end_in_line = if li == el { ec } else { self.state.lines[li].len() };
                            let hl_start = sel_start_in_line.max(chunk_start).saturating_sub(chunk_start);
                            let hl_end = sel_end_in_line.min(chunk_start + ll.text.len()).saturating_sub(chunk_start);

                            if hl_start < hl_end && hl_end <= ll.text.len() {
                                // Build the line char by char with selection and cursor
                                display = Self::render_line_with_selection_and_cursor(
                                    &ll.text, pos, hl_start, hl_end, marker,
                                );
                            } else if !raw_after.is_empty() {
                                let first_char: String = raw_after.chars().next().map(|c| c.to_string()).unwrap_or_default();
                                let rest = &raw_after[first_char.len()..];
                                display = format!(
                                    "{}{}\x1b[7m{}\x1b[0m{}",
                                    raw_before, marker, first_char, rest
                                );
                            } else {
                                display = format!(
                                    "{}{}\x1b[7m \x1b[0m",
                                    raw_before, marker
                                );
                            }
                        } else if !raw_after.is_empty() {
                            let first_char: String = raw_after.chars().next().map(|c| c.to_string()).unwrap_or_default();
                            let rest = &raw_after[first_char.len()..];
                            display = format!(
                                "{}{}\x1b[7m{}\x1b[0m{}",
                                raw_before, marker, first_char, rest
                            );
                        } else {
                            display = format!(
                                "{}{}\x1b[7m \x1b[0m",
                                raw_before, marker
                            );
                        }
                    } else if !raw_after.is_empty() {
                        let first_char: String = raw_after.chars().next().map(|c| c.to_string()).unwrap_or_default();
                        let rest = &raw_after[first_char.len()..];
                        display = format!(
                            "{}{}\x1b[7m{}\x1b[0m{}",
                            raw_before, marker, first_char, rest
                        );
                    } else {
                        display = format!(
                            "{}{}\x1b[7m \x1b[0m",
                            raw_before, marker
                        );
                    }
                }
            }

            // Pad to full width
            let vw = visible_width(&display);
            let padding = if w > vw { " ".repeat(w - vw) } else { String::new() };
            result.push(format!("{}{}", display, padding));
        }

        // Bottom border (with scroll indicator if more below)
        let lines_below = layout_lines.len().saturating_sub(scroll + visible.len());
        if lines_below > 0 {
            let indicator = format!("─── ↓ {} more ", lines_below);
            let remaining = w.saturating_sub(visible_width(&indicator));
            result.push(format!(
                "{}{}{}{}",
                self.border_color,
                indicator,
                "─".repeat(remaining),
                "\x1b[0m"
            ));
        } else {
            result.push(border.clone());
        }

        if let Some(menu) = &self.slash_menu {
            let menu_lines = menu.render(width);
            result.extend(menu_lines);
        }

        if let Some(menu) = &self.file_menu {
            let menu_lines = menu.render(width);
            result.extend(menu_lines);
        }

        result
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        let KeyEvent { code, modifiers, .. } = *key;

        if let Some(menu) = &mut self.file_menu {
            match (code, modifiers) {
                (KeyCode::Up, _) | (KeyCode::Down, _) | (KeyCode::PageUp, _) | (KeyCode::PageDown, _) | (KeyCode::Home, _) | (KeyCode::End, _) => {
                    let _ = menu.handle_key(*key);
                    return;
                }
                (KeyCode::Esc, _) => {
                    self.file_menu = None;
                    return;
                }
                (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Tab, KeyModifiers::NONE) => {
                    match menu.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
                        SelectAction::Selected(value) => {
                            self.accept_file_selection(value);
                        }
                        SelectAction::Cancelled | SelectAction::None => {}
                    }
                    return;
                }
                _ => {}
            }
        }

        if let Some(menu) = &mut self.slash_menu {
            match (code, modifiers) {
                (KeyCode::Up, _) | (KeyCode::Down, _) | (KeyCode::PageUp, _) | (KeyCode::PageDown, _) | (KeyCode::Home, _) | (KeyCode::End, _) => {
                    let _ = menu.handle_key(*key);
                    return;
                }
                (KeyCode::Esc, _) => {
                    self.slash_menu = None;
                    return;
                }
                (KeyCode::Enter, KeyModifiers::NONE) | (KeyCode::Tab, KeyModifiers::NONE) => {
                    match menu.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
                        SelectAction::Selected(value) => {
                            self.accept_slash_selection(value);
                        }
                        SelectAction::Cancelled | SelectAction::None => {}
                    }
                    return;
                }
                _ => {}
            }
        }

        let old_action = std::mem::replace(&mut self.last_action, LastAction::Other);
// Helper: check if shift is held (for selection extension)
        let shift = modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let _alt = modifiers.contains(KeyModifiers::ALT);
        let ctrl_shift = ctrl && shift;

        match (code, modifiers) {
            // Submit (Enter, no modifiers)
            (KeyCode::Enter, KeyModifiers::NONE) => {
                // Handled externally via try_submit()
            }

            // Newline (Alt+Enter, Shift+Enter)
            (KeyCode::Enter, KeyModifiers::ALT) |
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                self.push_undo();
self.clear_selection();
                self.new_line();
            }

            // Select all (Ctrl+A)
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.select_all();
            }

            // Copy (Ctrl+C)
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.has_selection() {
                    if let Some(text) = self.selected_text() {
                        // Try to copy to system clipboard via OSC 52
                        let encoded = base64_encode(&text);
                        let osc = format!("\x1b]52;c;{}\x07", encoded);
                        let _ = std::io::Write::write_all(&mut std::io::stdout(), osc.as_bytes());
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                }
            }

            // Cut (Ctrl+X)
            (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                if self.has_selection() {
                    if let Some(text) = self.selected_text() {
                        let encoded = base64_encode(&text);
                        let osc = format!("\x1b]52;c;{}\x07", encoded);
                        let _ = std::io::Write::write_all(&mut std::io::stdout(), osc.as_bytes());
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                    self.delete_selection();
                }
            }

            // Shift+arrow selection
            (KeyCode::Left, _) if ctrl_shift => {
                self.ensure_anchor();
                self.word_left();
            }
            (KeyCode::Right, _) if ctrl_shift => {
                self.ensure_anchor();
                self.word_right();
            }
            (KeyCode::Left, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_left();
            }
            (KeyCode::Right, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_right();
            }
            (KeyCode::Up, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_up();
            }
            (KeyCode::Down, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_down();
            }
            (KeyCode::Home, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_to_line_start();
            }
            (KeyCode::End, KeyModifiers::SHIFT) => {
                self.ensure_anchor();
                self.move_to_line_end();
            }

            // Cursor movement (clear selection)
            (KeyCode::Left, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_left();
            }
            (KeyCode::Right, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_right();
            }
            (KeyCode::Up, KeyModifiers::NONE) => {
                self.clear_selection();
                if self.is_empty() || self.is_on_first_visual_line() {
                    self.navigate_history(-1);
                } else {
                    self.move_up();
                }
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                self.clear_selection();
                if self.history_index > -1 && self.is_on_last_visual_line() {
                    self.navigate_history(1);
                } else {
                    self.move_down();
                }
            }

            // Home/End
            (KeyCode::Home, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_to_line_start();
            }
            (KeyCode::Char('e'), KeyModifiers::CONTROL) |
            (KeyCode::End, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_to_line_end();
            }

            // Word jump
            (KeyCode::Left, KeyModifiers::CONTROL) |
            (KeyCode::Left, KeyModifiers::ALT) => {
                self.clear_selection();
                self.word_left();
            }
            (KeyCode::Right, KeyModifiers::CONTROL) |
            (KeyCode::Right, KeyModifiers::ALT) => {
                self.clear_selection();
                self.word_right();
            }

            // Deletion
            (KeyCode::Backspace, KeyModifiers::NONE) |
            (KeyCode::Backspace, KeyModifiers::SHIFT) => {
                self.push_undo();
                self.backspace();
            }
            (KeyCode::Delete, _) => {
                self.push_undo();
                self.delete();
            }
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                self.push_undo();
                let accumulate = old_action == LastAction::Kill;
                self.kill_to_end(accumulate);
                self.last_action = LastAction::Kill;
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.push_undo();
                let accumulate = old_action == LastAction::Kill;
                self.kill_to_start(accumulate);
                self.last_action = LastAction::Kill;
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) |
            (KeyCode::Backspace, KeyModifiers::ALT) => {
                self.push_undo();
                let accumulate = old_action == LastAction::Kill;
                self.delete_word_backward(accumulate);
                self.last_action = LastAction::Kill;
            }

            // Yank
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                self.yank();
            }
            // Yank-pop
            (KeyCode::Char('y'), KeyModifiers::ALT) => {
                self.last_action = old_action;
                self.yank_pop();
            }

            // Undo
            (KeyCode::Char('z'), KeyModifiers::CONTROL) => {
                self.undo();
            }
            // Redo (Ctrl+Shift+Z)
            // Redo (Ctrl+Shift+Z)
            (KeyCode::Char('Z'), KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
                self.redo();
            }

            // Regular characters
            (KeyCode::Char(c), KeyModifiers::NONE) |
            (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                self.push_undo();
                self.delete_selection();
                self.insert_char(c);
            }

            // Tab -> 4 spaces
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.push_undo();
                self.delete_selection();
                self.insert_str("    ");
            }

            // Select all
            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.state.selection_anchor = Some((0, 0));
                let last_line = self.state.lines.len() - 1;
                self.state.cursor_line = last_line;
                self.state.cursor_col = self.state.lines[last_line].len();
            }

            _ => {}
        }

        self.update_slash_menu();
        self.update_file_menu();
    }

    fn handle_raw_input(&mut self, data: &str) {
        // Handle bracketed paste
        self.push_undo();
        for c in data.chars() {
            if c == '\n' || c == '\r' {
                self.new_line();
            } else if c >= ' ' {
                self.insert_char(c);
            }
        }
        self.last_action = LastAction::Other;
        self.update_slash_menu();
        self.update_file_menu();
    }

    fn invalidate(&mut self) {}
}

impl Focusable for Editor {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

impl Editor {
    /// Try to take a submitted value. Called from the event loop after Enter.
    pub fn try_submit(&mut self) -> Option<String> {
        if self.slash_menu.is_some() || self.file_menu.is_some() {
            return None;
        }
        self.submit()
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_editor_empty() {
        let editor = Editor::new();
        assert_eq!(editor.get_text(), "");
    }

    #[test]
    fn test_set_text() {
        let mut editor = Editor::new();
        editor.set_text("hello\nworld");
        assert_eq!(editor.get_text(), "hello\nworld");
    }

    #[test]
    fn test_insert_char() {
        let mut editor = Editor::new();
        editor.insert_char('h');
        editor.insert_char('i');
        assert_eq!(editor.get_text(), "hi");
    }

    #[test]
    fn test_backspace() {
        let mut editor = Editor::new();
        editor.set_text("hello");
        editor.backspace();
        assert_eq!(editor.get_text(), "hell");
    }

    #[test]
    fn test_backspace_empty() {
        let mut editor = Editor::new();
        editor.backspace(); // should not panic
        assert_eq!(editor.get_text(), "");
    }

    #[test]
    fn test_new_line() {
        let mut editor = Editor::new();
        editor.set_text("hello");
        // Move cursor to middle
        editor.state.cursor_col = 2;
        editor.new_line();
        assert_eq!(editor.get_text(), "he\nllo");
    }

    #[test]
    fn test_submit() {
        let mut editor = Editor::new();
        editor.set_text("hello world");
        let result = editor.try_submit();
        assert_eq!(result, Some("hello world".to_string()));
        assert_eq!(editor.get_text(), "");
    }

    #[test]
    fn test_submit_empty() {
        let mut editor = Editor::new();
        let result = editor.try_submit();
        assert_eq!(result, None);
    }

    #[test]
    fn test_history() {
        let mut editor = Editor::new();
        editor.add_to_history("first");
        editor.add_to_history("second");
        editor.navigate_history(-1); // up -> most recent
        assert_eq!(editor.get_text(), "second");
        editor.navigate_history(-1); // up -> older
        assert_eq!(editor.get_text(), "first");
        editor.navigate_history(1); // down -> back to recent
        assert_eq!(editor.get_text(), "second");
    }

    #[test]
    fn test_render_bordered() {
        let mut editor = Editor::new();
        editor.set_text("hello");
        <Editor as Focusable>::set_focused(&mut editor, true);
        let lines = editor.render(40);
        // Should have: top border, content line, bottom border
        assert!(lines.len() >= 3, "Expected at least 3 lines, got {}", lines.len());
        // Top border should contain ─
        assert!(lines[0].contains("─"), "Top border missing");
        // Last line should contain ─
        assert!(lines.last().unwrap().contains("─"), "Bottom border missing");
    }

    #[test]
    fn test_render_cursor_marker() {
        let mut editor = Editor::new();
        editor.set_text("hi");
        <Editor as Focusable>::set_focused(&mut editor, true);
        let lines = editor.render(40);
        let joined = lines.join("");
        assert!(joined.contains(CURSOR_MARKER), "Should contain cursor marker when focused");
    }

    #[test]
    fn test_render_no_cursor_when_unfocused() {
        let editor = Editor::new();
        let lines = editor.render(40);
        let joined = lines.join("");
        assert!(!joined.contains(CURSOR_MARKER), "Should not contain cursor marker when unfocused");
    }

    #[test]
    fn test_word_wrap_line() {
        let chunks = Editor::word_wrap_line("hello world foo", 10);
        assert!(chunks.len() >= 2, "Should wrap, got {} chunks", chunks.len());
    }

    #[test]
    fn test_slash_menu_shows_on_slash() {
        let mut editor = Editor::new();
        editor.insert_char('/');
        editor.update_slash_menu();
        assert!(editor.is_showing_slash_menu());
    }

    #[test]
    fn test_slash_menu_hides_after_space() {
        let mut editor = Editor::new();
        editor.set_text("/model foo");
        assert!(!editor.is_showing_slash_menu());
    }

    #[test]
    fn test_slash_menu_render_contains_commands() {
        let mut editor = Editor::new();
        editor.insert_char('/');
        editor.update_slash_menu();
        let lines = editor.render(80);
        let joined = lines.join("\n");
        assert!(joined.contains("/help") || joined.contains("/model"));
    }
}
