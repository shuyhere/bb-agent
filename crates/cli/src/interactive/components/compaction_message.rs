use std::any::Any;

use bb_tui::component::Component;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionSummaryMessage {
    pub summary: String,
    pub tokens_before: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionSummaryMessageComponent {
    expanded: bool,
    message: CompactionSummaryMessage,
}

impl CompactionSummaryMessageComponent {
    pub fn new(summary: impl Into<String>, tokens_before: usize) -> Self {
        Self {
            expanded: false,
            message: CompactionSummaryMessage {
                summary: summary.into(),
                tokens_before,
            },
        }
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }

    pub fn expanded(&self) -> bool {
        self.expanded
    }

    pub fn render_lines(&self) -> Vec<String> {
        let token_str = self.message.tokens_before.to_string();
        let mut lines = vec!["[compaction]".to_string(), String::new()];

        if self.expanded {
            lines.push(format!("Compacted from {token_str} tokens"));
            lines.push(String::new());
            lines.extend(self.message.summary.lines().map(|line| line.to_string()));
        } else {
            lines.push(format!(
                "Compacted from {token_str} tokens (expand to view)"
            ));
        }

        lines
    }

    pub fn render_plain_text(&self) -> String {
        self.render_lines().join("\n")
    }
}

impl Component for CompactionSummaryMessageComponent {
    fn render(&self, _width: u16) -> Vec<String> {
        self.render_lines()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
