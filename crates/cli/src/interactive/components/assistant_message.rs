use std::any::Any;
use std::cell::{Cell, RefCell};

use bb_tui::component::Component;
use bb_tui::markdown::MarkdownRenderer;
use bb_tui::theme::theme;

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
    /// Cached rendered lines. Invalidated on content/settings change.
    cached_lines: RefCell<Option<Vec<String>>>,
    cached_width: Cell<u16>,
}

impl Default for AssistantMessageComponent {
    fn default() -> Self {
        Self {
            hide_thinking_block: false,
            hidden_thinking_label: "Thinking...".to_string(),
            last_message: None,
            cached_lines: RefCell::new(None),
            cached_width: Cell::new(0),
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
        *self.cached_lines.get_mut() = None;
    }

    pub fn set_hidden_thinking_label(&mut self, label: impl Into<String>) {
        self.hidden_thinking_label = label.into();
        *self.cached_lines.get_mut() = None;
    }

    pub fn update_content(&mut self, message: AssistantMessage) {
        self.last_message = Some(message);
        *self.cached_lines.get_mut() = None;
    }

    pub fn last_message(&self) -> Option<&AssistantMessage> {
        self.last_message.as_ref()
    }

    pub fn render_lines(&self, width: u16) -> Vec<String> {
        if let Some(ref cached) = *self.cached_lines.borrow() {
            if self.cached_width.get() == width {
                return cached.clone();
            }
        }
        let lines = self.render_lines_inner(width);
        *self.cached_lines.borrow_mut() = Some(lines.clone());
        self.cached_width.set(width);
        lines
    }

    fn render_lines_inner(&self, width: u16) -> Vec<String> {
        let Some(message) = &self.last_message else {
            return Vec::new();
        };

        let width = width.max(1);
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
                    lines.extend(render_markdown_lines(text.trim(), width));
                }
                AssistantMessageContent::Thinking(thinking) if !thinking.trim().is_empty() => {
                    let has_visible_content_after = message.content[index + 1..].iter().any(|next| {
                        matches!(next, AssistantMessageContent::Text(text) if !text.trim().is_empty())
                            || matches!(next, AssistantMessageContent::Thinking(text) if !text.trim().is_empty())
                    });

                    if self.hide_thinking_block {
                        let t = theme();
                        lines.push(apply_line_style(&self.hidden_thinking_label, &[&t.italic, &t.thinking_text]));
                    } else {
                        let t = theme();
                        let thinking_lines = render_markdown_lines(thinking.trim(), width);
                        lines.extend(
                            thinking_lines
                                .into_iter()
                                .map(|line| apply_line_style(&line, &[&t.italic, &t.thinking_text])),
                        );
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
                    let t = theme();
                    let abort_message = match message.error_message.as_deref() {
                        Some(message) if message != "Request was aborted" => message.to_string(),
                        _ => "Operation aborted".to_string(),
                    };
                    lines.push(String::new());
                    lines.push(apply_line_style(&abort_message, &[&t.error]));
                }
                Some(AssistantStopReason::Error) => {
                    let t = theme();
                    let error_message = message.error_message.as_deref().unwrap_or("Unknown error");
                    lines.push(String::new());
                    lines.push(apply_line_style(&format!("Error: {error_message}"), &[&t.error]));
                }
                _ => {}
            }
        }

        lines
    }

    pub fn render_plain_text(&self) -> String {
        self.render_lines_inner(80).join("\n")
    }
}

impl Component for AssistantMessageComponent {
    fn render(&self, width: u16) -> Vec<String> {
        self.render_lines(width)
    }

    fn invalidate(&mut self) {
        *self.cached_lines.get_mut() = None;
        self.cached_width.set(0);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn render_markdown_lines(text: &str, width: u16) -> Vec<String> {
    let mut renderer = MarkdownRenderer::new(text);
    renderer.render(width)
}

fn apply_line_style(line: &str, styles: &[&str]) -> String {
    let t = theme();
    let style_prefix = styles.join("");
    if line.is_empty() {
        return style_prefix + &t.reset;
    }
    let reset = &t.reset;
    let reapplied = line.replace(reset.as_str(), &format!("{reset}{style_prefix}"));
    format!("{style_prefix}{reapplied}{reset}")
}
