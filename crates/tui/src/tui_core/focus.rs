use super::TUI;

impl TUI {
    /// Set focus to a specific child index, calling `set_focused()` on old/new.
    pub fn set_focus(&mut self, index: Option<usize>) {
        if let Some(old_idx) = self.focus_index
            && let Some(old) = self.root.children.get_mut(old_idx)
        {
            old.set_focused(false);
        }
        self.focus_index = index;
        if let Some(new_idx) = index
            && let Some(new_comp) = self.root.children.get_mut(new_idx)
        {
            new_comp.set_focused(true);
        }
    }

    /// Get the currently focused child index.
    pub fn focus_index(&self) -> Option<usize> {
        self.focus_index
    }
}
