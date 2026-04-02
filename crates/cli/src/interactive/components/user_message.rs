use std::any::Any;

use bb_tui::component::Component;
use bb_tui::theme::theme;
use bb_tui::utils::word_wrap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessageComponent {
    text: String,
}

impl UserMessageComponent {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    pub fn render_lines(&self, width: u16) -> Vec<String> {
        if self.text.trim().is_empty() {
            return Vec::new();
        }

        let t = theme();
        let wrap_width = width.saturating_sub(1).max(1) as usize;
        let mut lines = vec![String::new()];
        for line in self.text.lines() {
            for wrapped in word_wrap(line, wrap_width) {
                lines.push(format!("{} {}\x1b[K{}", t.user_msg_bg, wrapped, t.reset));
            }
        }
        lines.push(String::new());
        lines
    }

    pub fn render_plain_text(&self) -> String {
        self.text.clone()
    }
}

impl Component for UserMessageComponent {
    fn render(&self, width: u16) -> Vec<String> {
        self.render_lines(width)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
