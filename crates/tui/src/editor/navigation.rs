use super::types::Editor;

impl Editor {
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
}
