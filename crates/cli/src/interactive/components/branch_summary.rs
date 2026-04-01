#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryMessage {
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchSummaryMessageComponent {
    expanded: bool,
    message: BranchSummaryMessage,
}

impl BranchSummaryMessageComponent {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            expanded: false,
            message: BranchSummaryMessage {
                summary: summary.into(),
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
        let mut lines = vec!["[branch]".to_string(), String::new()];

        if self.expanded {
            lines.push("Branch Summary".to_string());
            lines.push(String::new());
            lines.extend(self.message.summary.lines().map(|line| line.to_string()));
        } else {
            lines.push("Branch summary (expand to view)".to_string());
        }

        lines
    }

    pub fn render_plain_text(&self) -> String {
        self.render_lines().join("\n")
    }
}
