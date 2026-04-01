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

    pub fn render_lines(&self) -> Vec<String> {
        if self.text.trim().is_empty() {
            return Vec::new();
        }

        let mut lines = vec![String::new()];
        lines.extend(self.text.lines().map(|line| line.to_string()));
        lines
    }

    pub fn render_plain_text(&self) -> String {
        self.render_lines().join("\n")
    }
}
