use bb_tui::component::{Component, Focusable};
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

const BORDER_COLOR: &str = "\x1b[38;2;178;148;187m";
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";

impl Component for SessionSelectorOverlay {
    fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();
        let border = format!("{BORDER_COLOR}{}{RESET}", "\u{2500}".repeat(w));

        lines.push(border.clone());
        lines.push(format!("  {BOLD}Resume Session{RESET}"));

        if !self.search.is_empty() {
            lines.push(format!("  {DIM}Search: {RESET}{}", self.search));
        }
        lines.push(String::new());

        let filtered = self.filtered();
        if filtered.is_empty() {
            if self.sessions.is_empty() {
                lines.push(format!("  {DIM}No sessions found{RESET}"));
            } else {
                lines.push(format!("  {DIM}No matching sessions{RESET}"));
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
                    format!(" {GREEN}(current){RESET}")
                } else {
                    String::new()
                };

                let age = format_age(&item.updated_at);
                let display_name = item
                    .name
                    .as_deref()
                    .unwrap_or(&item.session_id[..8.min(item.session_id.len())]);
                let short_cwd = shorten_path(&item.cwd);
                let msgs = item.entry_count;

                let line = if is_selected {
                    format!(
                        "  {BORDER_COLOR}>{RESET} {BOLD}{display_name}{RESET}{current_marker}  {DIM}{short_cwd}  {age}  {msgs} msgs{RESET}"
                    )
                } else {
                    format!(
                        "    {display_name}{current_marker}  {DIM}{short_cwd}  {age}  {msgs} msgs{RESET}"
                    )
                };

                let vis = visible_width(&line);
                let pad = w.saturating_sub(vis);
                lines.push(format!("{line}{}", " ".repeat(pad)));
            }

            if filtered.len() > max_show {
                lines.push(format!(
                    "  {DIM}... {} more sessions{RESET}",
                    filtered.len() - max_show
                ));
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "{DIM}  Enter: resume  Esc: cancel  Type to search{RESET}"
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
