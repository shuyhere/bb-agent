use bb_session::store::SessionRow;
use crossterm::event::KeyEvent;

use crate::select_list::{SelectAction, SelectItem, SelectList};

/// Result of session selection.
pub struct SessionSelection {
    pub session_id: String,
    pub name: Option<String>,
    pub entry_count: i64,
}

/// Session selector overlay using SelectList.
pub struct SessionSelector {
    list: SelectList,
    sessions: Vec<SessionRow>,
}

impl SessionSelector {
    /// Create a new session selector from session rows.
    pub fn new(sessions: Vec<SessionRow>, max_visible: usize) -> Self {
        let items: Vec<SelectItem> = sessions
            .iter()
            .map(|s| {
                let label = s
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Session {}", &s.session_id[..8.min(s.session_id.len())]));
                let detail = format!(
                    "{} entries · updated {}",
                    s.entry_count,
                    format_timestamp(&s.updated_at),
                );
                SelectItem {
                    label,
                    detail: Some(detail),
                    value: s.session_id.clone(),
                }
            })
            .collect();

        Self {
            list: SelectList::new(items, max_visible),
            sessions,
        }
    }

    /// Render the selector.
    pub fn render(&self, width: u16) -> Vec<String> {
        let mut lines = vec![
            format!("Resume Session"),
            format!(""),
        ];
        lines.extend(self.list.render(width));
        lines
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Result<SessionSelection, ()>> {
        match self.list.handle_key(key) {
            SelectAction::None => None,
            SelectAction::Cancelled => Some(Err(())),
            SelectAction::Selected(session_id) => {
                if let Some(session) = self.sessions.iter().find(|s| s.session_id == session_id) {
                    Some(Ok(SessionSelection {
                        session_id: session.session_id.clone(),
                        name: session.name.clone(),
                        entry_count: session.entry_count,
                    }))
                } else {
                    Some(Err(()))
                }
            }
        }
    }

    /// Update the search filter.
    pub fn set_search(&mut self, query: &str) {
        self.list.set_search(query);
    }
}

/// Format a timestamp string for display (show relative or short form).
fn format_timestamp(ts: &str) -> String {
    // Try to parse RFC3339 and show a short form
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
        let now = chrono::Utc::now();
        let diff = now.signed_duration_since(dt);

        if diff.num_minutes() < 1 {
            "just now".to_string()
        } else if diff.num_hours() < 1 {
            format!("{}m ago", diff.num_minutes())
        } else if diff.num_hours() < 24 {
            format!("{}h ago", diff.num_hours())
        } else if diff.num_days() < 7 {
            format!("{}d ago", diff.num_days())
        } else {
            dt.format("%Y-%m-%d").to_string()
        }
    } else {
        // Fallback: just show first 16 chars
        ts.chars().take(16).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sessions() -> Vec<SessionRow> {
        vec![
            SessionRow {
                session_id: "aaaa-bbbb-cccc-dddd".into(),
                cwd: "/tmp".into(),
                created_at: "2026-03-30T10:00:00Z".into(),
                updated_at: "2026-03-31T15:30:00Z".into(),
                name: Some("refactor-tui".into()),
                leaf_id: Some("abc123".into()),
                entry_count: 42,
            },
            SessionRow {
                session_id: "eeee-ffff-1111-2222".into(),
                cwd: "/tmp".into(),
                created_at: "2026-03-29T08:00:00Z".into(),
                updated_at: "2026-03-30T12:00:00Z".into(),
                name: None,
                leaf_id: Some("def456".into()),
                entry_count: 7,
            },
        ]
    }

    #[test]
    fn test_session_selector_creation() {
        let selector = SessionSelector::new(make_sessions(), 10);
        let lines = selector.render(80);
        assert!(lines[0].contains("Resume Session"));
    }

    #[test]
    fn test_session_selector_select() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut selector = SessionSelector::new(make_sessions(), 10);
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let result = selector.handle_key(key);
        assert!(result.is_some());
        let selection = result.unwrap().unwrap();
        assert_eq!(selection.session_id, "aaaa-bbbb-cccc-dddd");
        assert_eq!(selection.name.as_deref(), Some("refactor-tui"));
        assert_eq!(selection.entry_count, 42);
    }

    #[test]
    fn test_session_selector_escape() {
        use crossterm::event::{KeyCode, KeyModifiers};

        let mut selector = SessionSelector::new(make_sessions(), 10);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let result = selector.handle_key(key);
        assert!(matches!(result, Some(Err(()))));
    }

    #[test]
    fn test_format_timestamp() {
        let ts = "2026-03-31T15:30:00+00:00";
        let formatted = format_timestamp(ts);
        assert!(!formatted.is_empty());
    }

    #[test]
    fn test_empty_sessions() {
        let selector = SessionSelector::new(vec![], 10);
        let lines = selector.render(80);
        assert!(!lines.is_empty());
    }
}
