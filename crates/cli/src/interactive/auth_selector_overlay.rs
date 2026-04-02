use crate::login::{self, AuthSource};
use bb_tui::component::{Component, Focusable};
use bb_tui::theme::theme;
use bb_tui::utils::visible_width;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::any::Any;

/// Result from the auth selector overlay.
#[derive(Debug, Clone)]
pub enum AuthSelectorAction {
    /// User selected a provider to login/re-auth.
    Login(String),
    /// User selected a provider to logout / remove credentials.
    Logout(String),
    Cancelled,
    Pending,
}

/// Mode for the auth selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSelectorMode {
    Login,
    Logout,
}

/// Whether a provider authenticates via OAuth or a pasted API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    OAuth,
    ApiKey,
}

impl AuthMethod {
    pub fn label(self) -> &'static str {
        match self {
            AuthMethod::OAuth => "OAuth",
            AuthMethod::ApiKey => "API key",
        }
    }
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
    auth_method: AuthMethod,
    source: Option<AuthSource>,
}

const PROVIDERS: &[(&str, &str, AuthMethod)] = &[
    ("anthropic", "Anthropic", AuthMethod::OAuth),
    ("openai-codex", "OpenAI Codex", AuthMethod::OAuth),
    ("google", "Google", AuthMethod::ApiKey),
    ("groq", "Groq", AuthMethod::ApiKey),
    ("xai", "xAI", AuthMethod::ApiKey),
    ("openrouter", "OpenRouter", AuthMethod::ApiKey),
];

impl AuthSelectorOverlay {
    pub fn new(mode: AuthSelectorMode) -> Self {
        let providers: Vec<ProviderEntry> = PROVIDERS
            .iter()
            .map(|(id, display, method)| ProviderEntry {
                name: id.to_string(),
                display: display.to_string(),
                auth_method: *method,
                source: login::auth_source(id),
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

    /// Returns the auth method of the currently selected provider.
    pub fn selected_auth_method(&self) -> Option<AuthMethod> {
        self.providers.get(self.selected).map(|e| e.auth_method)
    }
}

/// Look up the auth method for a provider by its id.
pub fn auth_method_for(provider: &str) -> AuthMethod {
    PROVIDERS
        .iter()
        .find(|(id, _, _)| *id == provider)
        .map(|(_, _, m)| *m)
        .unwrap_or(AuthMethod::ApiKey)
}

/// Human-friendly provider display name.
pub fn auth_display_name_for(provider: &str) -> &str {
    PROVIDERS
        .iter()
        .find(|(id, _, _)| *id == provider)
        .map(|(_, display, _)| *display)
        .unwrap_or(provider)
}

impl Component for AuthSelectorOverlay {
    fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();

        let t = theme();
        let border_color = &t.accent;
        let reset = &t.reset;
        let dim = &t.dim;
        let bold = &t.bold;
        let green = &t.green;
        let yellow = &t.yellow;
        let border = format!("{border_color}{}{reset}", "\u{2500}".repeat(w));

        lines.push(border.clone());

        // Title
        let title = match self.mode {
            AuthSelectorMode::Login => "Select provider to login / re-auth:",
            AuthSelectorMode::Logout => "Select provider to logout:",
        };
        lines.push(format!("  {bold}{title}{reset}"));
        lines.push(String::new());

        // Provider list
        for (i, entry) in self.providers.iter().enumerate() {
            let is_selected = i == self.selected;

            let method_tag = format!(" ({})", entry.auth_method.label());

            let status = match entry.source {
                Some(src) => format!(
                    "{method_tag} {green}[via {}]{reset}",
                    src.label()
                ),
                None => format!("{method_tag} {dim}[not authenticated]{reset}"),
            };

            let reauth_hint = if is_selected && entry.source.is_some() && self.mode == AuthSelectorMode::Login {
                format!("  {yellow}(Enter to re-auth){reset}")
            } else {
                String::new()
            };

            let line = if is_selected {
                format!(
                    "  {border_color}>{reset} {bold}{}{reset}{status}{reauth_hint}",
                    entry.display
                )
            } else {
                format!("    {}{status}", entry.display)
            };

            let vis = visible_width(&line);
            let pad = w.saturating_sub(vis);
            lines.push(format!("{line}{}", " ".repeat(pad)));
        }

        lines.push(String::new());

        // Hints
        let hint = match self.mode {
            AuthSelectorMode::Login => {
                format!("{dim}  Enter: login/re-auth  Esc: cancel{reset}")
            }
            AuthSelectorMode::Logout => {
                format!("{dim}  Enter: logout  Esc: cancel{reset}")
            }
        };
        lines.push(hint);

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
                    self.action = match self.mode {
                        AuthSelectorMode::Login => {
                            AuthSelectorAction::Login(entry.name.clone())
                        }
                        AuthSelectorMode::Logout => {
                            AuthSelectorAction::Logout(entry.name.clone())
                        }
                    };
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
