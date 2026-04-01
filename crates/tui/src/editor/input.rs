use super::types::{Editor, EditorSnapshot, LastAction};
use crate::fuzzy::fuzzy_filter;
use crate::select_list::{SelectAction, SelectItem, SelectList};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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

impl Editor {
    pub(super) fn slash_query(&self) -> Option<String> {
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

    pub(super) fn update_slash_menu(&mut self) {
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

    pub(super) fn accept_slash_selection(&mut self, value: String) {
        self.state.lines[0] = value;
        self.state.cursor_line = 0;
        self.state.cursor_col = self.state.lines[0].len();
        self.slash_menu = None;
    }

    /// Detect `@query` before the cursor. Returns the full `@...` token if found.
    pub(super) fn file_query(&self) -> Option<String> {
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
    pub(super) fn scan_files(
        dir: &std::path::Path,
        base: &std::path::Path,
        depth: usize,
        max_depth: usize,
        results: &mut Vec<String>,
    ) {
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
            if path.is_dir()
                && matches!(
                    name.as_str(),
                    "node_modules" | "target" | "dist" | "build" | ".git" | "__pycache__"
                )
            {
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

    pub(super) fn update_file_menu(&mut self) {
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

    pub(super) fn accept_file_selection(&mut self, path: String) {
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

    pub(super) fn insert_char(&mut self, c: char) {
        self.history_index = -1;
        if self.has_selection() {
            self.delete_selection();
        }
        let line = &mut self.state.lines[self.state.cursor_line];
        line.insert(self.state.cursor_col, c);
        self.state.cursor_col += c.len_utf8();
    }

    pub(super) fn insert_str(&mut self, s: &str) {
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

    pub(super) fn backspace(&mut self) {
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

    pub(super) fn delete(&mut self) {
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

    pub(super) fn new_line(&mut self) {
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

    pub(super) fn move_left(&mut self) {
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

    pub(super) fn move_right(&mut self) {
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

    pub(super) fn move_up(&mut self) {
        if self.state.cursor_line > 0 {
            self.state.cursor_line -= 1;
            self.state.cursor_col = self
                .state
                .cursor_col
                .min(self.state.lines[self.state.cursor_line].len());
        }
    }

    pub(super) fn move_down(&mut self) {
        if self.state.cursor_line + 1 < self.state.lines.len() {
            self.state.cursor_line += 1;
            self.state.cursor_col = self
                .state
                .cursor_col
                .min(self.state.lines[self.state.cursor_line].len());
        }
    }

    pub(super) fn move_to_line_start(&mut self) {
        self.state.cursor_col = 0;
    }

    pub(super) fn move_to_line_end(&mut self) {
        self.state.cursor_col = self.state.lines[self.state.cursor_line].len();
    }

    pub(super) fn kill_to_end(&mut self, accumulate: bool) {
        let line = &self.state.lines[self.state.cursor_line];
        let killed = line[self.state.cursor_col..].to_string();
        if !killed.is_empty() {
            self.kill_ring.push(&killed, false, accumulate);
        }
        self.state.lines[self.state.cursor_line].truncate(self.state.cursor_col);
    }

    pub(super) fn kill_to_start(&mut self, accumulate: bool) {
        let line = &self.state.lines[self.state.cursor_line];
        let killed = line[..self.state.cursor_col].to_string();
        if !killed.is_empty() {
            self.kill_ring.push(&killed, true, accumulate);
        }
        let rest = line[self.state.cursor_col..].to_string();
        self.state.lines[self.state.cursor_line] = rest;
        self.state.cursor_col = 0;
    }

    pub(super) fn word_left(&mut self) {
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

    pub(super) fn word_right(&mut self) {
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

    pub(super) fn delete_word_backward(&mut self, accumulate: bool) {
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

    pub(super) fn navigate_history(&mut self, direction: isize) {
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

    pub(super) fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            lines: self.state.lines.clone(),
            cursor_line: self.state.cursor_line,
            cursor_col: self.state.cursor_col,
        }
    }

    pub(super) fn restore(&mut self, snap: EditorSnapshot) {
        self.state.lines = snap.lines;
        self.state.cursor_line = snap.cursor_line;
        self.state.cursor_col = snap.cursor_col;
    }

    pub(super) fn push_undo(&mut self) {
        let snap = self.snapshot();
        self.undo_stack.push(&snap);
        self.redo_stack.clear();
    }

    pub(super) fn undo(&mut self) {
        if let Some(snap) = self.undo_stack.pop() {
            let current = self.snapshot();
            self.redo_stack.push(&current);
            self.restore(snap);
        }
    }

    pub(super) fn redo(&mut self) {
        if let Some(snap) = self.redo_stack.pop() {
            let current = self.snapshot();
            self.undo_stack.push(&current);
            self.restore(snap);
        }
    }

    // ── Yank ──

    pub(super) fn yank(&mut self) {
        if let Some(text) = self.kill_ring.peek().map(|s| s.to_string()) {
            self.push_undo();
            let len = text.len();
            let line = &mut self.state.lines[self.state.cursor_line];
            line.insert_str(self.state.cursor_col, &text);
            self.state.cursor_col += len;
            self.last_action = LastAction::Yank { len };
        }
    }

    pub(super) fn yank_pop(&mut self) {
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

    pub(super) fn submit(&mut self) -> Option<String> {
        let text = self.get_text().trim().to_string();
        if text.is_empty() {
            return None;
        }
        self.clear();
        self.history_index = -1;
        Some(text)
    }

    pub(super) fn handle_key_event(&mut self, key: &KeyEvent) {
        let KeyEvent {
            code, modifiers, ..
        } = *key;

        if let Some(menu) = &mut self.file_menu {
            match (code, modifiers) {
                (KeyCode::Up, _)
                | (KeyCode::Down, _)
                | (KeyCode::PageUp, _)
                | (KeyCode::PageDown, _)
                | (KeyCode::Home, _)
                | (KeyCode::End, _) => {
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
                (KeyCode::Up, _)
                | (KeyCode::Down, _)
                | (KeyCode::PageUp, _)
                | (KeyCode::PageDown, _)
                | (KeyCode::Home, _)
                | (KeyCode::End, _) => {
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
            (KeyCode::Enter, KeyModifiers::ALT) | (KeyCode::Enter, KeyModifiers::SHIFT) => {
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
            (KeyCode::Char('e'), KeyModifiers::CONTROL) | (KeyCode::End, KeyModifiers::NONE) => {
                self.clear_selection();
                self.move_to_line_end();
            }

            // Word jump
            (KeyCode::Left, KeyModifiers::CONTROL) | (KeyCode::Left, KeyModifiers::ALT) => {
                self.clear_selection();
                self.word_left();
            }
            (KeyCode::Right, KeyModifiers::CONTROL) | (KeyCode::Right, KeyModifiers::ALT) => {
                self.clear_selection();
                self.word_right();
            }

            // Deletion
            (KeyCode::Backspace, KeyModifiers::NONE)
            | (KeyCode::Backspace, KeyModifiers::SHIFT) => {
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
            (KeyCode::Char('w'), KeyModifiers::CONTROL)
            | (KeyCode::Backspace, KeyModifiers::ALT) => {
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
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
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

            _ => {}
        }

        self.update_slash_menu();
        self.update_file_menu();
    }

    pub(super) fn handle_paste_input(&mut self, data: &str) {
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
