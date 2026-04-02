use super::types::Editor;

impl Editor {
    pub(super) fn has_selection(&self) -> bool {
        if let Some((al, ac)) = self.state.selection_anchor {
            al != self.state.cursor_line || ac != self.state.cursor_col
        } else {
            false
        }
    }

    /// Returns ordered (start, end) positions of the selection.
    pub(super) fn selection_range(&self) -> Option<((usize, usize), (usize, usize))> {
        let (al, ac) = self.state.selection_anchor?;
        let (cl, cc) = (self.state.cursor_line, self.state.cursor_col);
        if al == cl && ac == cc {
            return None;
        }
        let start = if al < cl || (al == cl && ac < cc) {
            (al, ac)
        } else {
            (cl, cc)
        };
        let end = if al < cl || (al == cl && ac < cc) {
            (cl, cc)
        } else {
            (al, ac)
        };
        Some((start, end))
    }

    /// Get the selected text.
    pub(super) fn selected_text(&self) -> Option<String> {
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
    pub(super) fn delete_selection(&mut self) {
        let Some(((sl, sc), (el, ec))) = self.selection_range() else {
            return;
        };
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

    pub(super) fn clear_selection(&mut self) {
        self.state.selection_anchor = None;
    }

    /// Select all text in the editor.
    pub(super) fn select_all(&mut self) {
        self.state.selection_anchor = Some((0, 0));
        let last = self.state.lines.len() - 1;
        self.state.cursor_line = last;
        self.state.cursor_col = self.state.lines[last].len();
    }

    /// Set the anchor if starting a new selection, or keep existing anchor.
    pub(super) fn ensure_anchor(&mut self) {
        if self.state.selection_anchor.is_none() {
            self.state.selection_anchor = Some((self.state.cursor_line, self.state.cursor_col));
        }
    }
}
