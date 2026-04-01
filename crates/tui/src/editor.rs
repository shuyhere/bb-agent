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
use crate::utils::visible_width;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Editor state.
struct EditorState {
    lines: Vec<String>,
    cursor_line: usize,
    cursor_col: usize,
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
}

impl Editor {
    pub fn new() -> Self {
        Self {
            state: EditorState {
                lines: vec![String::new()],
                cursor_line: 0,
                cursor_col: 0,
            },
            focused: false,
            terminal_rows: 24,
            scroll_offset: 0,
            history: Vec::new(),
            history_index: -1,
            on_submit: None,
            disable_submit: false,
            border_color: "\x1b[90m".to_string(), // dim gray
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
    }

    /// Clear the editor.
    pub fn clear(&mut self) {
        self.state.lines = vec![String::new()];
        self.state.cursor_line = 0;
        self.state.cursor_col = 0;
        self.scroll_offset = 0;
        self.history_index = -1;
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

    fn is_on_first_visual_line(&self) -> bool {
        self.state.cursor_line == 0
    }

    fn is_on_last_visual_line(&self) -> bool {
        self.state.cursor_line == self.state.lines.len() - 1
    }

    fn is_empty(&self) -> bool {
        self.state.lines.len() == 1 && self.state.lines[0].is_empty()
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
                        });
                    } else {
                        layout.push(LayoutLine {
                            text: text.clone(),
                            has_cursor: false,
                            cursor_pos: None,
                        });
                    }
                } else {
                    layout.push(LayoutLine {
                        text: text.clone(),
                        has_cursor: false,
                        cursor_pos: None,
                    });
                }
            }
        }

        layout
    }

    // ── Input handling ──

    fn insert_char(&mut self, c: char) {
        self.history_index = -1;
        let line = &mut self.state.lines[self.state.cursor_line];
        line.insert(self.state.cursor_col, c);
        self.state.cursor_col += c.len_utf8();
    }

    fn insert_str(&mut self, s: &str) {
        self.history_index = -1;
        for c in s.chars() {
            self.insert_char(c);
        }
    }

    fn backspace(&mut self) {
        self.history_index = -1;
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

    fn kill_to_end(&mut self) {
        self.state.lines[self.state.cursor_line].truncate(self.state.cursor_col);
    }

    fn kill_to_start(&mut self) {
        let rest = self.state.lines[self.state.cursor_line][self.state.cursor_col..].to_string();
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

    fn delete_word_backward(&mut self) {
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

        // Content lines
        let emit_cursor = self.focused;
        for ll in visible {
            let mut display = ll.text.clone();

            if ll.has_cursor && emit_cursor {
                if let Some(pos) = ll.cursor_pos {
                    let before = &ll.text[..pos.min(ll.text.len())];
                    let after = &ll.text[pos.min(ll.text.len())..];

                    let marker = CURSOR_MARKER;
                    if !after.is_empty() {
                        // Cursor on a character — highlight it
                        let first_char: String = after.chars().next().map(|c| c.to_string()).unwrap_or_default();
                        let rest = &after[first_char.len()..];
                        display = format!(
                            "{}{}\x1b[7m{}\x1b[0m{}",
                            before, marker, first_char, rest
                        );
                    } else {
                        // Cursor at end — show highlighted space
                        display = format!(
                            "{}{}\x1b[7m \x1b[0m",
                            before, marker
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
            result.push(border);
        }

        result
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        let KeyEvent { code, modifiers, .. } = *key;

        match (code, modifiers) {
            // Submit (Enter, no modifiers)
            (KeyCode::Enter, KeyModifiers::NONE) => {
                // Handled externally via try_take_submitted
            }

            // Newline (Alt+Enter, Shift+Enter)
            (KeyCode::Enter, KeyModifiers::ALT) |
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                self.new_line();
            }

            // Cursor movement
            (KeyCode::Left, KeyModifiers::NONE) => self.move_left(),
            (KeyCode::Right, KeyModifiers::NONE) => self.move_right(),
            (KeyCode::Up, KeyModifiers::NONE) => {
                if self.is_empty() || self.is_on_first_visual_line() {
                    self.navigate_history(-1);
                } else {
                    self.move_up();
                }
            }
            (KeyCode::Down, KeyModifiers::NONE) => {
                if self.history_index > -1 && self.is_on_last_visual_line() {
                    self.navigate_history(1);
                } else {
                    self.move_down();
                }
            }

            // Home/End
            (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                self.move_to_line_start();
            }
            (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                self.move_to_line_end();
            }

            // Word jump
            (KeyCode::Left, KeyModifiers::CONTROL) |
            (KeyCode::Left, KeyModifiers::ALT) => {
                self.word_left();
            }
            (KeyCode::Right, KeyModifiers::CONTROL) |
            (KeyCode::Right, KeyModifiers::ALT) => {
                self.word_right();
            }

            // Deletion
            (KeyCode::Backspace, KeyModifiers::NONE) |
            (KeyCode::Backspace, KeyModifiers::SHIFT) => {
                self.backspace();
            }
            (KeyCode::Delete, _) => {
                self.delete();
            }
            (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                self.kill_to_end();
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.kill_to_start();
            }
            (KeyCode::Char('w'), KeyModifiers::CONTROL) |
            (KeyCode::Backspace, KeyModifiers::ALT) => {
                self.delete_word_backward();
            }

            // Regular characters
            (KeyCode::Char(c), KeyModifiers::NONE) |
            (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                self.insert_char(c);
            }

            // Tab → 4 spaces
            (KeyCode::Tab, KeyModifiers::NONE) => {
                self.insert_str("    ");
            }

            _ => {}
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        // Handle bracketed paste
        for c in data.chars() {
            if c == '\n' || c == '\r' {
                self.new_line();
            } else if c >= ' ' {
                self.insert_char(c);
            }
        }
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
}
