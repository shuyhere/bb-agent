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

pub struct LoginDialogComponent {
    provider_name: String,
    url: Option<String>,
    message: Option<String>,
    editor: Editor,
    pending_action: Option<LoginDialogAction>,
    focused: bool,
}

impl LoginDialogComponent {
    pub fn new(provider_name: impl Into<String>) -> Self {
        let mut editor = Editor::new();
        editor.border_color = theme().accent.clone();
        Focusable::set_focused(&mut editor, true);
        Self {
            provider_name: provider_name.into(),
            url: None,
            message: None,
            editor,
            pending_action: None,
            focused: true,
        }
    }

    pub fn set_url(&mut self, url: Option<String>) {
        self.url = url;
    }

    pub fn set_message(&mut self, message: Option<String>) {
        self.message = message;
    }

    pub fn clear_input(&mut self) {
        self.editor.clear();
    }

    pub fn take_action(&mut self) -> Option<LoginDialogAction> {
        self.pending_action.take()
    }
}

impl Component for LoginDialogComponent {
    fn render(&self, width: u16) -> Vec<String> {
        let t = theme();
        let width = width.max(20) as usize;
        let border = format!("{}{}{}", t.accent, "─".repeat(width), t.reset);
        let mut lines = vec![
            border.clone(),
            format!(" {}Login to {}{}{}", t.yellow, t.bold, self.provider_name, t.reset),
        ];

        if let Some(url) = &self.url {
            lines.push(String::new());
            lines.push(format!(" {}{}{}", t.accent, url, t.reset));
        }

        if let Some(message) = &self.message {
            lines.push(String::new());
            for line in message.lines() {
                lines.push(format!(" {}{}{}", t.dim, line, t.reset));
            }
        }

        lines.push(String::new());
        let inner_width = width.saturating_sub(2) as u16;
        for line in self.editor.render(inner_width) {
            lines.push(format!(" {line}"));
        }

        lines.push(String::new());
        lines.push(format!(" {}Enter to submit  Esc to cancel{}", t.dim, t.reset));
        lines.push(border);
        lines
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.pending_action = Some(LoginDialogAction::Cancelled);
            }
            (KeyCode::Enter, modifiers) if !modifiers.contains(KeyModifiers::SHIFT) => {
                self.pending_action = Some(LoginDialogAction::Submit(self.editor.get_text()));
            }
            _ => self.editor.handle_input(key),
        }
    }

    fn handle_raw_input(&mut self, data: &str) {
        self.editor.handle_raw_input(data);
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
