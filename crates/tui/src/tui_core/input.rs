use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{InputResult, TUI};

impl TUI {
    /// Add an input listener that can intercept/transform input before it
    /// reaches the focused component.
    pub fn add_input_listener(&mut self, listener: Box<dyn Fn(&str) -> InputResult + Send>) {
        self.input_listeners.push(listener);
    }

    /// Dispatch a key event to the focused component (overlay or root child).
    /// Input listeners run first; if any returns `Consumed` dispatch is skipped.
    pub fn handle_key(&mut self, key: &KeyEvent) {
        let key_str = format!("{key:?}");
        for listener in &self.input_listeners {
            match listener(&key_str) {
                InputResult::Consumed => return,
                InputResult::Transformed(_) | InputResult::Ignored => {}
            }
        }

        if key
            .modifiers
            .contains(KeyModifiers::SHIFT | KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('D')
            && let Some(ref mut callback) = self.on_debug
        {
            callback();
            return;
        }

        if let Some(entry) = self.topmost_capturing_overlay_mut() {
            entry.component.handle_input(key);
            return;
        }
        if let Some(idx) = self.focus_index
            && let Some(child) = self.root.children.get_mut(idx)
        {
            child.handle_input(key);
        }
    }

    /// Dispatch raw input (e.g. paste) to the focused component.
    /// Input listeners run first and may consume or transform the input.
    pub fn handle_raw_input(&mut self, data: &str) {
        let mut current = data.to_string();
        for listener in &self.input_listeners {
            match listener(&current) {
                InputResult::Consumed => return,
                InputResult::Transformed(new) => current = new,
                InputResult::Ignored => {}
            }
        }

        if let Some(entry) = self.topmost_capturing_overlay_mut() {
            entry.component.handle_raw_input(&current);
            return;
        }
        if let Some(idx) = self.focus_index
            && let Some(child) = self.root.children.get_mut(idx)
        {
            child.handle_raw_input(&current);
        }
    }
}
