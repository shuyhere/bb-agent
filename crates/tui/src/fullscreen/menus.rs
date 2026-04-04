use crate::select_list::{SelectItem, SelectList};
use crate::slash_commands::shared_slash_command_select_items;

use super::runtime::FullscreenState;

#[derive(Clone, Debug)]
pub(crate) struct FullscreenSlashMenuState {
    all_items: Vec<SelectItem>,
    pub(super) list: SelectList,
}

#[derive(Clone, Debug)]
pub(super) struct FullscreenSelectMenuState {
    pub(super) menu_id: String,
    title: String,
    pub(super) list: SelectList,
}

fn colorize_tree_menu_label(label: &str) -> String {
    let t = crate::theme::theme();
    label
        .replace("[U]", &format!("{}[U]{}", t.cyan, t.reset))
        .replace("[A]", &format!("{}[A]{}", t.green, t.reset))
        .replace("[T]", &format!("{}[T]{}", t.yellow, t.reset))
        .replace("[C]", &format!("{}[C]{}", t.dim, t.reset))
        .replace("[B]", &format!("{}[B]{}", t.accent, t.reset))
        .replace("[?]", &format!("{}[?]{}", t.dim, t.reset))
}

impl FullscreenSelectMenuState {
    pub(super) fn new(menu_id: String, title: String, mut items: Vec<SelectItem>) -> Self {
        if menu_id == "tree-entry" {
            for item in &mut items {
                item.label = colorize_tree_menu_label(&item.label);
            }
        }
        let mut list = SelectList::new(items, 16);
        list.set_show_search(false);
        Self { menu_id, title, list }
    }

    pub(super) fn selected_value(&self) -> Option<String> {
        self.list.selected_value()
    }

    pub(super) fn render(&self, width: usize) -> Vec<String> {
        let mut lines = vec![crate::utils::pad_to_width(
            &crate::utils::truncate_to_width(
                &format!("{} (Enter select, Esc close)", self.title),
                width,
            ),
            width,
        )];
        lines.extend(
            self.list
                .render(width as u16)
                .into_iter()
                .map(|line| crate::utils::pad_to_width(&crate::utils::truncate_to_width(&line, width), width)),
        );
        lines
    }

    pub(super) fn rendered_height(&self) -> u16 {
        self.render(80).len() as u16
    }
}

impl FullscreenSlashMenuState {
    pub(crate) fn new(extra_items: &[SelectItem]) -> Self {
        let mut all_items = shared_slash_command_select_items();
        // Add skill, prompt, and extension command items
        all_items.extend(extra_items.iter().cloned());
        let mut list = SelectList::new(all_items.clone(), 6);
        list.set_show_search(false);
        Self { all_items, list }
    }

    pub(super) fn set_search(&mut self, query: &str) {
        let q = query.trim_start_matches('/').to_ascii_lowercase();
        let items = self
            .all_items
            .iter()
            .filter(|item| {
                if q.is_empty() {
                    true
                } else {
                    item.label
                        .trim_start_matches('/')
                        .to_ascii_lowercase()
                        .starts_with(&q)
                        || item
                            .value
                            .trim_start_matches('/')
                            .to_ascii_lowercase()
                            .starts_with(&q)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut list = SelectList::new(items, 6);
        list.set_show_search(false);
        self.list = list;
    }

    pub(crate) fn selected_value(&self) -> Option<String> {
        self.list.selected_value()
    }

    pub(super) fn render(&self, width: usize) -> Vec<String> {
        let mut lines = self
            .list
            .render(width as u16)
            .into_iter()
            .map(|line| line.replace(" items", " commands"))
            .collect::<Vec<_>>();
        if lines.is_empty() {
            lines.push("  (no matching commands)".to_string());
        }
        lines
    }

    pub(super) fn rendered_height(&self) -> u16 {
        self.render(80).len() as u16
    }
}

impl FullscreenState {
    pub(crate) fn render_select_menu_lines(&self, width: usize) -> Option<Vec<String>> {
        self.select_menu.as_ref().map(|menu| menu.render(width))
    }

    pub(crate) fn render_slash_menu_lines(&self, width: usize) -> Option<Vec<String>> {
        self.slash_menu.as_ref().map(|menu| menu.render(width))
    }

    pub(super) fn slash_query(&self) -> Option<String> {
        let before = self.input.get(..self.cursor)?;
        if before.contains('\n') {
            return None;
        }
        if !before.starts_with('/') {
            return None;
        }
        if before.contains(' ') {
            return None;
        }
        Some(before.to_string())
    }

    pub(super) fn update_slash_menu(&mut self) {
        let Some(query) = self.slash_query() else {
            self.slash_menu = None;
            return;
        };
        let extra = self.extra_slash_items.clone();
        let mut menu = self.slash_menu.take().unwrap_or_else(|| FullscreenSlashMenuState::new(&extra));
        menu.set_search(&query);
        self.slash_menu = Some(menu);
    }

    pub(super) fn accept_slash_selection(&mut self, value: String) {
        self.input = value;
        self.cursor = self.input.len();
        self.slash_menu = None;
        self.dirty = true;
    }
}
