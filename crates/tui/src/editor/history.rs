use super::types::{Editor, EditorSnapshot, LastAction};

impl Editor {
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
}
