use crate::login;
use bb_tui::component::{Component, Focusable};
use bb_tui::utils::visible_width;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::any::Any;

/// Result from the auth selector overlay.
#[derive(Debug, Clone)]
pub enum AuthSelectorAction {
    Selected(String), // provider name
    Cancelled,
    Pending,
}

/// Mode for the auth selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSelectorMode {
    Login,
    Logout,
}

/// An overlay component that shows available providers for login/logout.
pub struct AuthSelectorOverlay {
    mode: AuthSelectorMode,
    providers: Vec<ProviderEntry>,
    selected: usize,
    action: AuthSelectorAction,
    focused: bool,
}

struct ProviderEntry {
    name: String,
    display: String,
    has_auth: bool,
}

const PROVIDERS: &[(&str, &str)] = &[
    ("anthropic", "Anthropic"),
    ("openai", "OpenAI"),
    ("google", "Google"),
    ("groq", "Groq"),
    ("xai", "xAI"),
    ("openrouter", "OpenRouter"),
];

impl AuthSelectorOverlay {
    pub fn new(mode: AuthSelectorMode) -> Self {
        let providers: Vec<ProviderEntry> = PROVIDERS
            .iter()
            .map(|(id, display)| {
                let has_auth = login::provider_has_auth(id);
                ProviderEntry {
                    name: id.to_string(),
                    display: display.to_string(),
                    has_auth,
                }
            })
            .collect();

        Self {
            mode,
            providers,
            selected: 0,
            action: AuthSelectorAction::Pending,
            focused: true,
        }
    }

    pub fn action(&self) -> &AuthSelectorAction {
        &self.action
    }
}

impl Component for AuthSelectorOverlay {
    fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();

        // Border
        let border_color = "\x1b[38;2;178;148;187m";
        let reset = "\x1b[0m";
        let border = format!("{border_color}{}{reset}", "\u{2500}".repeat(w));
        lines.push(border.clone());

        // Title
        let title = match self.mode {
            AuthSelectorMode::Login => "Select provider to login:",
            AuthSelectorMode::Logout => "Select provider to logout:",
        };
        let title_line = format!("  \x1b[1m{title}\x1b[0m");
        lines.push(title_line);
        lines.push(String::new());

        // Provider list
        if self.providers.is_empty() {
            let msg = match self.mode {
                AuthSelectorMode::Login => "  No providers available",
                AuthSelectorMode::Logout => "  No providers logged in. Use /login first.",
            };
            lines.push(format!("\x1b[2m{msg}\x1b[0m"));
        } else {
            for (i, entry) in self.providers.iter().enumerate() {
                let is_selected = i == self.selected;
                let status = if entry.has_auth {
                    "\x1b[32m [authenticated]\x1b[0m"
                } else {
                    ""
                };

                let line = if is_selected {
                    format!(
                        "  {border_color}> \x1b[1m{}\x1b[0m{status}",
                        entry.display
                    )
                } else {
                    format!("    {}{status}", entry.display)
                };

                // Pad to width
                let vis = visible_width(&line);
                let pad = w.saturating_sub(vis);
                lines.push(format!("{line}{}", " ".repeat(pad)));
            }
        }

        lines.push(String::new());

        // Hint
        let hint = "\x1b[2m  Enter: select  Esc: cancel\x1b[0m";
        lines.push(hint.to_string());

        // Bottom border
        lines.push(border);

        lines
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        match (key.code, key.modifiers) {
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                if self.selected + 1 < self.providers.len() {
                    self.selected += 1;
                }
            }
            (KeyCode::Enter, _) => {
                if let Some(entry) = self.providers.get(self.selected) {
                    self.action = AuthSelectorAction::Selected(entry.name.clone());
                }
            }
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.action = AuthSelectorAction::Cancelled;
            }
            _ => {}
        }
    }

    fn invalidate(&mut self) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Focusable for AuthSelectorOverlay {
    fn focused(&self) -> bool {
        self.focused
    }
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}
