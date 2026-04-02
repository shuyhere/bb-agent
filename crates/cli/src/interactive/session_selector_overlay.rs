use bb_tui::component::{Component, Focusable};
use bb_tui::theme::theme;
use bb_tui::utils::visible_width;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::any::Any;

/// Result from the session selector.
#[derive(Debug, Clone)]
pub enum SessionSelectorAction {
    Selected(String), // session_id
    Cancelled,
    Pending,
}

/// A session entry for display.
#[derive(Clone)]
pub struct SessionListItem {
    pub session_id: String,
    pub name: Option<String>,
    pub cwd: String,
    pub updated_at: String,
    pub entry_count: i64,
    pub is_current: bool,
    /// First user message text (used as display name when no name set).
    pub first_message: String,
    /// All user+assistant message text concatenated (for search).
    pub all_messages_text: String,
}

pub struct SessionSelectorOverlay {
    sessions: Vec<SessionListItem>,
    selected: usize,
    action: SessionSelectorAction,
    focused: bool,
    search: String,
}

impl SessionSelectorOverlay {
    pub fn new(sessions: Vec<SessionListItem>) -> Self {
        Self {
            sessions,
            selected: 0,
            action: SessionSelectorAction::Pending,
            focused: true,
            search: String::new(),
        }
    }

    pub fn action(&self) -> &SessionSelectorAction {
        &self.action
    }

    fn filtered(&self) -> Vec<(usize, &SessionListItem)> {
        if self.search.is_empty() {
            self.sessions.iter().enumerate().collect()
        } else {
            let q = self.search.to_lowercase();
            self.sessions
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.session_id.to_lowercase().contains(&q)
                        || s.cwd.to_lowercase().contains(&q)
                        || s.name
                            .as_deref()
                            .map(|n| n.to_lowercase().contains(&q))
                            .unwrap_or(false)
                        || s.first_message.to_lowercase().contains(&q)
                        || s.all_messages_text.to_lowercase().contains(&q)
                })
                .collect()
        }
    }
}

fn format_age(updated_at: &str) -> String {
    let Ok(dt) = chrono::NaiveDateTime::parse_from_str(updated_at, "%Y-%m-%d %H:%M:%S") else {
        return updated_at.to_string();
    };
    let then = dt.and_utc();
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(then);
    let mins = diff.num_minutes();
    if mins < 1 {
        return "now".into();
    }
    if mins < 60 {
        return format!("{mins}m");
    }
    let hours = diff.num_hours();
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = diff.num_days();
    if days < 7 {
        return format!("{days}d");
    }
    if days < 30 {
        return format!("{}w", days / 7);
    }
    if days < 365 {
        return format!("{}mo", days / 30);
    }
    format!("{}y", days / 365)
}

fn shorten_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if path.starts_with(&home) {
            return format!("~{}", &path[home.len()..]);
        }
    }
    path.to_string()
}

impl Component for SessionSelectorOverlay {
    fn render(&self, width: u16) -> Vec<String> {
        let t = theme();
        let w = width as usize;
        let mut lines = Vec::new();
        let border = format!("{}{}{}", t.accent, "\u{2500}".repeat(w), t.reset);

        lines.push(border.clone());
        lines.push(format!("  {}Resume Session{}", t.bold, t.reset));

        if !self.search.is_empty() {
            lines.push(format!("  {}Search: {}{}", t.dim, t.reset, self.search));
        }
        lines.push(String::new());

        let filtered = self.filtered();
        if filtered.is_empty() {
            if self.sessions.is_empty() {
                lines.push(format!("  {}No sessions found{}", t.dim, t.reset));
            } else {
                lines.push(format!("  {}No matching sessions{}", t.dim, t.reset));
            }
        } else {
            // Show max 15 items
            let max_show = 15.min(filtered.len());
            let start = if self.selected >= max_show {
                self.selected + 1 - max_show
            } else {
                0
            };

            for (display_idx, &(_, item)) in filtered.iter().skip(start).take(max_show).enumerate() {
                let actual_idx = start + display_idx;
                let is_selected = actual_idx == self.selected;
                let current_marker = if item.is_current {
                    format!(" {}(current){}", t.green, t.reset)
                } else {
                    String::new()
                };

                let age = format_age(&item.updated_at);
                let display_name = item
                    .name
                    .as_deref()
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| {
                        if item.first_message.is_empty() {
                            "(no messages)"
                        } else {
                            &item.first_message
                        }
                    });
                // Truncate long display names (char-safe for multibyte)
                let display_name: &str = if display_name.chars().count() > 60 {
                    // Find char boundary
                    let end = display_name.char_indices().nth(57).map(|(i, _)| i).unwrap_or(display_name.len());
                    &display_name[..end]
                } else {
                    display_name
                };
                let msgs = item.entry_count;

                let line = if is_selected {
                    format!(
                        "  {}>{} {}{display_name}{}{current_marker}  {}{age}  {msgs} msgs{}",
                        t.accent, t.reset, t.bold, t.reset, t.dim, t.reset
                    )
                } else {
                    format!(
                        "    {display_name}{current_marker}  {}{age}  {msgs} msgs{}",
                        t.dim, t.reset
                    )
                };

                let vis = visible_width(&line);
                let pad = w.saturating_sub(vis);
                lines.push(format!("{line}{}", " ".repeat(pad)));
            }

            if filtered.len() > max_show {
                lines.push(format!(
                    "  {}... {} more sessions{}",
                    t.dim, filtered.len() - max_show, t.reset
                ));
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "{}  Enter: resume  Esc: cancel  Type to search{}",
            t.dim, t.reset
        ));
        lines.push(border);
        lines
    }

    fn handle_input(&mut self, key: &KeyEvent) {
        let filtered_len = self.filtered().len();

        match (key.code, key.modifiers) {
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                if self.selected + 1 < filtered_len {
                    self.selected += 1;
                }
            }
            (KeyCode::Enter, _) => {
                let filtered = self.filtered();
                if let Some(&(_, item)) = filtered.get(self.selected) {
                    self.action = SessionSelectorAction::Selected(item.session_id.clone());
                }
            }
            (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.action = SessionSelectorAction::Cancelled;
            }
            (KeyCode::Backspace, _) => {
                self.search.pop();
                self.selected = 0;
            }
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.search.push(c);
                self.selected = 0;
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

impl Focusable for SessionSelectorOverlay {
    fn focused(&self) -> bool {
        self.focused
    }
    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}
