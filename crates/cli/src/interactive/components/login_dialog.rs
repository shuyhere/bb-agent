use std::any::Any;

use bb_tui::component::{Component, Focusable};
use bb_tui::editor::Editor;
use bb_tui::theme::theme;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone)]
pub enum LoginDialogAction {
    Submit(String),
    Cancelled,
}

#[derive(Debug, Clone, Copy)]
enum LineStyle {
    Text,
    Dim,
    Accent,
    Warning,
}

#[derive(Debug, Clone)]
struct BodyLine {
    style: LineStyle,
    text: String,
}

pub struct LoginDialogComponent {
    provider_name: String,
    body: Vec<BodyLine>,
    input_visible: bool,
    input_hint: Option<String>,
    editor: Editor,
    pending_action: Option<LoginDialogAction>,
    focused: bool,
}

impl LoginDialogComponent {
    pub fn new(provider_name: impl Into<String>) -> Self {
        let mut editor = Editor::new();
        editor.border_color = theme().border.clone();
        Focusable::set_focused(&mut editor, true);
        Self {
            provider_name: provider_name.into(),
            body: Vec::new(),
            input_visible: false,
            input_hint: None,
            editor,
            pending_action: None,
            focused: true,
        }
    }

    fn push_line(&mut self, style: LineStyle, text: impl Into<String>) {
        self.body.push(BodyLine {
            style,
            text: text.into(),
        });
    }

    pub fn show_auth(&mut self, url: &str, instructions: Option<&str>) {
        self.body.clear();
        self.push_line(LineStyle::Text, String::new());
        self.push_line(LineStyle::Accent, url.to_string());
        let click_hint = if cfg!(target_os = "macos") {
            format!("\x1b]8;;{url}\x07Cmd+click to open\x1b]8;;\x07")
        } else {
            format!("\x1b]8;;{url}\x07Ctrl+click to open\x1b]8;;\x07")
        };
        self.push_line(LineStyle::Dim, click_hint);
        if let Some(instructions) = instructions.filter(|s| !s.trim().is_empty()) {
            self.push_line(LineStyle::Text, String::new());
            self.push_line(LineStyle::Warning, instructions.to_string());
        }
    }

    pub fn show_manual_input(&mut self, prompt: &str) {
        self.push_line(LineStyle::Text, String::new());
        self.push_line(LineStyle::Dim, prompt.to_string());
        self.input_visible = true;
        self.input_hint = Some("(Esc to cancel)".to_string());
        self.editor.set_text("");
    }

    pub fn show_prompt(&mut self, message: &str, placeholder: Option<&str>) {
        self.push_line(LineStyle::Text, String::new());
        self.push_line(LineStyle::Text, message.to_string());
        if let Some(placeholder) = placeholder.filter(|s| !s.trim().is_empty()) {
            self.push_line(LineStyle::Dim, format!("e.g., {placeholder}"));
        }
        self.input_visible = true;
        self.input_hint = Some("(Esc to cancel, Enter to submit)".to_string());
        self.editor.set_text("");
    }

    pub fn show_waiting(&mut self, message: &str) {
        self.push_line(LineStyle::Text, String::new());
        self.push_line(LineStyle::Dim, message.to_string());
        self.input_visible = false;
        self.input_hint = Some("(Esc to cancel)".to_string());
    }

    pub fn show_progress(&mut self, message: &str) {
        self.push_line(LineStyle::Dim, message.to_string());
    }

    pub fn show_message(&mut self, message: &str) {
        self.body.clear();
        for (idx, line) in message.lines().enumerate() {
            if idx > 0 {
                self.push_line(LineStyle::Text, String::new());
            }
            self.push_line(LineStyle::Dim, line.to_string());
        }
    }

    pub fn clear_input(&mut self) {
        self.editor.clear();
    }

    pub fn take_action(&mut self) -> Option<LoginDialogAction> {
        self.pending_action.take()
    }

    fn render_body_line(&self, line: &BodyLine) -> String {
        let t = theme();
        match line.style {
            LineStyle::Text => format!(" {}", line.text),
            LineStyle::Dim => format!(" {}{}{}", t.dim, line.text, t.reset),
            LineStyle::Accent => format!(" {}{}{}", t.accent, line.text, t.reset),
            LineStyle::Warning => format!(" {}{}{}", t.warning, line.text, t.reset),
        }
    }
}

impl Component for LoginDialogComponent {
    fn render(&self, width: u16) -> Vec<String> {
        let t = theme();
        let width = width.max(20) as usize;
        let border = format!("{}{}{}", t.border, "─".repeat(width), t.reset);
        let mut lines = vec![
            border.clone(),
            format!(" {}Login to {}{}", t.warning, self.provider_name, t.reset),
        ];

        for line in &self.body {
            lines.push(self.render_body_line(line));
        }

        if self.input_visible {
            let inner_width = width.saturating_sub(2) as u16;
            for line in self.editor.render(inner_width) {
                lines.push(format!(" {line}"));
            }
        }

        if let Some(hint) = &self.input_hint {
            lines.push(format!(" {hint}"));
        }

        lines.push(border);
        lines
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.pending_action = Some(LoginDialogAction::Cancelled);
            }
            (KeyCode::Enter, modifiers)
                if self.input_visible && !modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.pending_action = Some(LoginDialogAction::Submit(self.editor.get_text()));
            }
            _ if self.input_visible => self.editor.handle_input(key),
            _ => {}
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        if self.input_visible {
            self.editor.handle_raw_input(data);
        }
    }

    fn invalidate(&mut self) {
        self.editor.invalidate();
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        Focusable::set_focused(&mut self.editor, focused);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Focusable for LoginDialogComponent {
    fn focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        Focusable::set_focused(&mut self.editor, focused);
    }
}
