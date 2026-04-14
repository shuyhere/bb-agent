use crate::kill_ring::KillRingUpdate;

use super::types::Editor;
use super::{KillContinuation, KillDirection};

impl Editor {
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

    pub(super) fn kill_to_end(&mut self, continuation: KillContinuation) {
        let line = &self.state.lines[self.state.cursor_line];
        let killed = line[self.state.cursor_col..].to_string();
        self.record_kill(&killed, continuation, KillDirection::Forward);
        self.state.lines[self.state.cursor_line].truncate(self.state.cursor_col);
    }

    pub(super) fn kill_to_start(&mut self, continuation: KillContinuation) {
        let line = &self.state.lines[self.state.cursor_line];
        let killed = line[..self.state.cursor_col].to_string();
        let rest = line[self.state.cursor_col..].to_string();
        self.record_kill(&killed, continuation, KillDirection::Backward);
        self.state.lines[self.state.cursor_line] = rest;
        self.state.cursor_col = 0;
    }

    pub(super) fn delete_word_backward(&mut self, continuation: KillContinuation) {
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
        let new_line = format!("{}{}", &line[..new_col], &line[self.state.cursor_col..]);
        self.record_kill(&killed, continuation, KillDirection::Backward);
        self.state.lines[self.state.cursor_line] = new_line;
        self.state.cursor_col = new_col;
    }

    fn record_kill(
        &mut self,
        killed: &str,
        continuation: KillContinuation,
        direction: KillDirection,
    ) {
        let update = match (continuation, direction) {
            (KillContinuation::NewEntry, _) => KillRingUpdate::Push(killed),
            (KillContinuation::Continue, KillDirection::Forward) => KillRingUpdate::Append(killed),
            (KillContinuation::Continue, KillDirection::Backward) => {
                KillRingUpdate::Prepend(killed)
            }
        };
        self.kill_ring.record(update);
    }
}
