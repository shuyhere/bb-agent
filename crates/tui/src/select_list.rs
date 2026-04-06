use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crossterm::style::{Attribute, Color, Stylize};
use unicode_width::UnicodeWidthStr;

/// An item in the select list.
#[derive(Clone, Debug)]
pub struct SelectItem {
    pub label: String,
    pub detail: Option<String>,
    pub value: String,
}

/// Action returned from key handling.
pub enum SelectAction {
    None,
    Selected(String),
    Cancelled,
}

/// A filterable, scrollable select list with keyboard navigation.
#[derive(Clone, Debug)]
pub struct SelectList {
    items: Vec<SelectItem>,
    filtered: Vec<usize>,
    selected: usize,
    scroll_offset: usize,
    max_visible: usize,
    search: String,
    show_search: bool,
}

impl SelectList {
    pub fn new(items: Vec<SelectItem>, max_visible: usize) -> Self {
        let filtered: Vec<usize> = (0..items.len()).collect();
        Self {
            items,
            filtered,
            selected: 0,
            scroll_offset: 0,
            max_visible,
            search: String::new(),
            show_search: true,
        }
    }

    pub fn set_show_search(&mut self, show: bool) {
        self.show_search = show;
    }

    /// Update the search query and re-filter.
    pub fn set_search(&mut self, query: &str) {
        self.search = query.to_string();
        self.refilter();
    }

    pub fn selected_value(&self) -> Option<String> {
        self.filtered
            .get(self.selected)
            .and_then(|idx| self.items.get(*idx))
            .map(|item| item.value.clone())
    }

    pub fn set_selected_value(&mut self, value: &str) {
        if let Some(filtered_index) = self
            .filtered
            .iter()
            .position(|idx| self.items.get(*idx).is_some_and(|item| item.value == value))
        {
            self.selected = filtered_index;
            self.adjust_scroll();
        }
    }

    /// Render the select list into styled lines.
    pub fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let mut lines = Vec::new();

        // Optional search bar
        if self.show_search && !self.search.is_empty() {
            let search_line = format!(" {} {}", "🔍", self.search.clone().with(Color::Yellow),);
            lines.push(search_line);
            lines.push(format!("{}", "─".repeat(w.min(60)).with(Color::DarkGrey)));
        }

        if self.filtered.is_empty() {
            lines.push(format!(
                "  {}",
                "(no matching items)"
                    .with(Color::DarkGrey)
                    .attribute(Attribute::Dim)
            ));
            return lines;
        }

        let total = self.filtered.len();
        let visible_count = total.min(self.max_visible);
        let end = (self.scroll_offset + visible_count).min(total);

        // Scroll-up indicator
        if self.scroll_offset > 0 {
            lines.push(format!(
                "  {} {} more above",
                "▲".with(Color::DarkGrey),
                self.scroll_offset.to_string().with(Color::DarkGrey),
            ));
        }

        let theme = crate::theme::theme();

        // Render visible items
        for vi in self.scroll_offset..end {
            let item_idx = self.filtered[vi];
            let item = &self.items[item_idx];
            let is_selected = vi == self.selected;

            let line = if is_selected {
                let label = strip_ansi(&item.label);
                let detail = item
                    .detail
                    .as_deref()
                    .map(strip_ansi)
                    .map(|d| format!(" {d}"))
                    .unwrap_or_default();
                format!("{}→ {}{}{}", theme.accent, label, detail, theme.reset)
            } else {
                let detail_part = match &item.detail {
                    Some(d) => format!(
                        " {}",
                        d.clone().with(Color::DarkGrey).attribute(Attribute::Dim)
                    ),
                    None => String::new(),
                };
                format!("  {}{}", item.label, detail_part)
            };

            // Truncate to width if needed
            let visible = UnicodeWidthStr::width(strip_ansi(&line).as_str());
            if visible > w && w > 3 {
                // Simple truncation: just use the line as-is; terminal will clip
                lines.push(line);
            } else {
                lines.push(line);
            }
        }

        // Scroll-down indicator
        if end < total {
            lines.push(format!(
                "  {} {} more below",
                "▼".with(Color::DarkGrey),
                (total - end).to_string().with(Color::DarkGrey),
            ));
        }

        // Footer: item count
        lines.push(format!(
            "{}",
            format!(" {}/{} items", self.filtered.len(), self.items.len())
                .with(Color::DarkGrey)
                .attribute(Attribute::Dim)
        ));

        lines
    }

    /// Handle a key event. Returns the resulting action.
    pub fn handle_key(&mut self, key: KeyEvent) -> SelectAction {
        match (key.code, key.modifiers) {
            // Navigation
            (KeyCode::Up, _) | (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                self.move_up(1);
                SelectAction::None
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                self.move_down(1);
                SelectAction::None
            }
            (KeyCode::PageUp, _) => {
                self.move_up(self.max_visible);
                SelectAction::None
            }
            (KeyCode::PageDown, _) => {
                self.move_down(self.max_visible);
                SelectAction::None
            }
            (KeyCode::Home, _) => {
                self.selected = 0;
                self.scroll_offset = 0;
                SelectAction::None
            }
            (KeyCode::End, _) => {
                if !self.filtered.is_empty() {
                    self.selected = self.filtered.len() - 1;
                    self.adjust_scroll();
                }
                SelectAction::None
            }

            // Select
            (KeyCode::Enter, _) => {
                if let Some(&idx) = self.filtered.get(self.selected) {
                    SelectAction::Selected(self.items[idx].value.clone())
                } else {
                    SelectAction::Cancelled
                }
            }

            // Cancel
            (KeyCode::Esc, _) => SelectAction::Cancelled,

            // Backspace in search
            (KeyCode::Backspace, _) => {
                if self.show_search && !self.search.is_empty() {
                    self.search.pop();
                    self.refilter();
                }
                SelectAction::None
            }

            // Type characters → filter
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                if self.show_search {
                    self.search.push(c);
                    self.refilter();
                }
                SelectAction::None
            }

            _ => SelectAction::None,
        }
    }

    fn move_up(&mut self, n: usize) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(n);
        self.adjust_scroll();
    }

    fn move_down(&mut self, n: usize) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + n).min(self.filtered.len() - 1);
        self.adjust_scroll();
    }

    fn adjust_scroll(&mut self) {
        // Ensure selected is visible
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + self.max_visible {
            self.scroll_offset = self.selected + 1 - self.max_visible;
        }
    }

    fn refilter(&mut self) {
        if self.search.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let query = self.search.to_lowercase();
            self.filtered = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    fuzzy_match(&item.label, &query) || fuzzy_match(&item.value, &query)
                })
                .map(|(i, _)| i)
                .collect();
        }
        // Reset selection
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

/// Simple fuzzy match: all query chars appear in order in the haystack.
fn fuzzy_match(haystack: &str, query: &str) -> bool {
    let hay = haystack.to_lowercase();
    let mut hay_chars = hay.chars();
    for qc in query.chars() {
        loop {
            match hay_chars.next() {
                Some(hc) if hc == qc => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

/// Strip ANSI escape sequences for width calculation.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_items(n: usize) -> Vec<SelectItem> {
        (0..n)
            .map(|i| SelectItem {
                label: format!("Item {i}"),
                detail: Some(format!("detail {i}")),
                value: format!("val-{i}"),
            })
            .collect()
    }

    #[test]
    fn test_new_select_list() {
        let list = SelectList::new(make_items(5), 3);
        assert_eq!(list.filtered.len(), 5);
        assert_eq!(list.selected, 0);
    }

    #[test]
    fn test_navigation() {
        let mut list = SelectList::new(make_items(10), 3);
        list.move_down(1);
        assert_eq!(list.selected, 1);
        list.move_down(1);
        assert_eq!(list.selected, 2);
        list.move_up(1);
        assert_eq!(list.selected, 1);
        list.move_up(100);
        assert_eq!(list.selected, 0);
        list.move_down(100);
        assert_eq!(list.selected, 9);
    }

    #[test]
    fn test_fuzzy_filter() {
        let items = vec![
            SelectItem {
                label: "Claude Sonnet 4".into(),
                detail: None,
                value: "cs4".into(),
            },
            SelectItem {
                label: "GPT-4o".into(),
                detail: None,
                value: "gpt4o".into(),
            },
            SelectItem {
                label: "Claude Opus 4".into(),
                detail: None,
                value: "co4".into(),
            },
        ];
        let mut list = SelectList::new(items, 10);
        list.set_search("clau");
        assert_eq!(list.filtered.len(), 2);
        list.set_search("opus");
        assert_eq!(list.filtered.len(), 1);
        list.set_search("");
        assert_eq!(list.filtered.len(), 3);
    }

    #[test]
    fn test_fuzzy_match() {
        assert!(fuzzy_match("Claude Sonnet", "clsn"));
        assert!(fuzzy_match("Claude Sonnet", "claude"));
        assert!(!fuzzy_match("GPT-4o", "claude"));
        assert!(fuzzy_match("GPT-4o", "g4"));
    }

    #[test]
    fn test_select_enter() {
        let mut list = SelectList::new(make_items(3), 10);
        list.move_down(1);
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        match list.handle_key(key) {
            SelectAction::Selected(v) => assert_eq!(v, "val-1"),
            _ => panic!("expected Selected"),
        }
    }

    #[test]
    fn test_escape_cancels() {
        let mut list = SelectList::new(make_items(3), 10);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(matches!(list.handle_key(key), SelectAction::Cancelled));
    }

    #[test]
    fn test_render_nonempty() {
        let list = SelectList::new(make_items(5), 3);
        let lines = list.render(80);
        assert!(!lines.is_empty());
        let first_item_line = &lines[0];
        assert!(strip_ansi(first_item_line).starts_with("→ Item 0"));
    }

    #[test]
    fn selected_item_uses_arrow_and_no_reverse_background() {
        let list = SelectList::new(make_items(3), 3);
        let line = &list.render(80)[0];
        let plain = strip_ansi(line);

        assert!(plain.starts_with("→ Item 0 detail 0"));
        assert!(!line.contains("\x1b[7m"));
    }

    #[test]
    fn test_scroll_indicators() {
        let mut list = SelectList::new(make_items(10), 3);
        // Move to bottom so scroll offset changes
        list.move_down(9);
        let lines = list.render(80);
        let joined = lines
            .iter()
            .map(|l| strip_ansi(l))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("more above"));
    }
}
