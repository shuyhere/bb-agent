#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssistantMessageContent {
    Text(String),
    Thinking(String),
    ToolCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantStopReason {
    Aborted,
    Error,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantMessage {
    pub content: Vec<AssistantMessageContent>,
    pub stop_reason: Option<AssistantStopReason>,
    pub error_message: Option<String>,
}

impl AssistantMessage {
    pub fn has_tool_calls(&self) -> bool {
        self.content
            .iter()
            .any(|content| matches!(content, AssistantMessageContent::ToolCall))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantMessageComponent {
    hide_thinking_block: bool,
    hidden_thinking_label: String,
    last_message: Option<AssistantMessage>,
}

impl Default for AssistantMessageComponent {
    fn default() -> Self {
        Self {
            hide_thinking_block: false,
            hidden_thinking_label: "Thinking...".to_string(),
            last_message: None,
        }
    }
}

impl AssistantMessageComponent {
    pub fn new(message: Option<AssistantMessage>, hide_thinking_block: bool) -> Self {
        Self {
            hide_thinking_block,
            last_message: message,
            ..Self::default()
        }
    }

    pub fn set_hide_thinking_block(&mut self, hide: bool) {
        self.hide_thinking_block = hide;
    }

    pub fn set_hidden_thinking_label(&mut self, label: impl Into<String>) {
        self.hidden_thinking_label = label.into();
    }

    pub fn update_content(&mut self, message: AssistantMessage) {
        self.last_message = Some(message);
    }

    pub fn render_lines(&self) -> Vec<String> {
        let Some(message) = &self.last_message else {
            return Vec::new();
        };

        let has_visible_content = message.content.iter().any(|content| match content {
            AssistantMessageContent::Text(text) => !text.trim().is_empty(),
            AssistantMessageContent::Thinking(thinking) => !thinking.trim().is_empty(),
            AssistantMessageContent::ToolCall => false,
        });

        let mut lines = Vec::new();

        if has_visible_content {
            lines.push(String::new());
        }

        for (index, content) in message.content.iter().enumerate() {
            match content {
                AssistantMessageContent::Text(text) if !text.trim().is_empty() => {
                    lines.extend(split_trimmed_lines(text));
                }
                AssistantMessageContent::Thinking(thinking) if !thinking.trim().is_empty() => {
                    let has_visible_content_after = message.content[index + 1..].iter().any(|next| {
                        matches!(next, AssistantMessageContent::Text(text) if !text.trim().is_empty())
                            || matches!(next, AssistantMessageContent::Thinking(text) if !text.trim().is_empty())
                    });

                    if self.hide_thinking_block {
                        lines.push(self.hidden_thinking_label.clone());
                    } else {
                        lines.extend(split_trimmed_lines(thinking));
                    }

                    if has_visible_content_after {
                        lines.push(String::new());
                    }
                }
                _ => {}
            }
        }

        if !message.has_tool_calls() {
            match message.stop_reason {
                Some(AssistantStopReason::Aborted) => {
                    let abort_message = match message.error_message.as_deref() {
                        Some(message) if message != "Request was aborted" => message.to_string(),
                        _ => "Operation aborted".to_string(),
                    };
                    lines.push(String::new());
                    lines.push(abort_message);
                }
                Some(AssistantStopReason::Error) => {
                    let error_message = message
                        .error_message
                        .as_deref()
                        .unwrap_or("Unknown error");
                    lines.push(String::new());
                    lines.push(format!("Error: {error_message}"));
                }
                _ => {}
            }
        }

        lines
    }

    pub fn render_plain_text(&self) -> String {
        self.render_lines().join("\n")
    }
}

fn split_trimmed_lines(value: &str) -> Vec<String> {
    value
        .trim()
        .lines()
        .map(|line| line.to_string())
        .collect()
}
