use std::path::Path;

use bb_session::{store::EntryRow, tree::TreeNode};

use crate::select_list::{SelectItem, SelectList};
use crate::slash_commands::shared_slash_command_select_items;
use crate::tree_selector::{TreeAction, TreeSelector, build_tree_selector};

use super::runtime::TuiState;

#[derive(Clone, Debug)]
pub(crate) struct TuiSlashMenuState {
    all_items: Vec<SelectItem>,
    pub(super) list: SelectList,
}

#[derive(Clone, Debug)]
pub(super) struct TuiSelectMenuState {
    pub(super) menu_id: String,
    title: String,
    pub(super) list: SelectList,
}

#[derive(Clone, Debug)]
pub(super) struct TuiTreeMenuState {
    pub(super) menu_id: String,
    selector: TreeSelector,
}

fn colorize_tree_menu_label(label: &str) -> String {
    let t = crate::theme::theme();
    label
        .replace("[U] you", &format!("{}you{}", t.cyan, t.reset))
        .replace("[A] agent", &format!("{}agent{}", t.green, t.reset))
        .replace("[T]", &t.dim.to_string())
        .replace("[C] compact", &format!("{}compact{}", t.dim, t.reset))
        .replace("[B] summary", &format!("{}summary{}", t.dim, t.reset))
        .replace("[?] other", &format!("{}other{}", t.dim, t.reset))
}

impl TuiSelectMenuState {
    pub(super) fn new(
        menu_id: String,
        title: String,
        mut items: Vec<SelectItem>,
        selected_value: Option<String>,
    ) -> Self {
        if menu_id == "tree-entry" {
            for item in &mut items {
                item.label = colorize_tree_menu_label(&item.label);
            }
        }
        let mut list = SelectList::new(items, 16);
        list.set_show_search(false);
        if let Some(value) = selected_value.as_deref() {
            list.set_selected_value(value);
        }
        Self {
            menu_id,
            title,
            list,
        }
    }

    pub(super) fn render(&self, width: usize) -> Vec<String> {
        let mut lines = vec![crate::utils::pad_to_width(
            &crate::utils::truncate_to_width(
                &format!("{} (Enter select, Esc close)", self.title),
                width,
            ),
            width,
        )];
        lines.extend(self.list.render(width as u16).into_iter().map(|line| {
            crate::utils::pad_to_width(&crate::utils::truncate_to_width(&line, width), width)
        }));
        lines
    }

    pub(super) fn rendered_height(&self) -> u16 {
        self.render(80).len() as u16
    }
}

impl TuiTreeMenuState {
    pub(super) fn new(
        menu_id: String,
        tree: Vec<TreeNode>,
        entries: Vec<EntryRow>,
        active_leaf: Option<String>,
        selected_value: Option<String>,
        max_visible: usize,
    ) -> Self {
        let mut selector = build_tree_selector(tree, &entries, active_leaf.as_deref(), max_visible);
        if let Some(value) = selected_value.as_deref() {
            selector.set_selected_value(value);
        }
        Self { menu_id, selector }
    }

    pub(super) fn selected_value(&self) -> Option<String> {
        self.selector.selected_value()
    }

    pub(super) fn set_max_visible(&mut self, max_visible: usize) {
        self.selector.set_max_visible(max_visible);
    }

    pub(super) fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> TreeAction {
        self.selector.handle_key(key)
    }

    pub(super) fn render(&self, width: usize) -> Vec<String> {
        self.selector
            .render(width as u16)
            .into_iter()
            .map(|line| {
                crate::utils::pad_to_width(&crate::utils::truncate_to_width(&line, width), width)
            })
            .collect()
    }
}

impl TuiSlashMenuState {
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

// =========================================================================
// @ file completion menu
// =========================================================================

#[derive(Clone, Debug)]
pub(crate) struct AtFileMenuState {
    pub(super) list: SelectList,
    /// The `@` prefix text that triggered this menu (e.g. `@src/m`).
    pub(super) at_prefix: String,
}

#[derive(Clone, Debug)]
struct AtQueryState {
    at_pos: usize,
    query: String,
    quoted: bool,
}

impl AtFileMenuState {
    /// Build file suggestions for the given `@` query.
    pub(crate) fn new(query: &str, cwd: &Path) -> Self {
        let items = list_file_suggestions(query, cwd, 64);
        let mut list = SelectList::new(items, 8);
        list.set_show_search(false);
        Self {
            list,
            at_prefix: format!("@{query}"),
        }
    }

    pub(crate) fn update(&mut self, query: &str, cwd: &Path) {
        let items = list_file_suggestions(query, cwd, 64);
        let mut list = SelectList::new(items, 8);
        list.set_show_search(false);
        self.list = list;
        self.at_prefix = format!("@{query}");
    }

    pub(crate) fn selected_value(&self) -> Option<String> {
        self.list.selected_value()
    }

    pub(super) fn rendered_height(&self) -> u16 {
        self.render(80).len() as u16
    }

    pub(super) fn render(&self, width: usize) -> Vec<String> {
        let lines = self.list.render(width as u16);
        if lines.is_empty() {
            vec!["  (no matching files)".to_string()]
        } else {
            lines
        }
    }
}

/// List files in `cwd` matching `query` prefix.
fn list_file_suggestions(query: &str, cwd: &Path, max: usize) -> Vec<SelectItem> {
    if query.is_empty() || query.contains('/') {
        return list_files_via_readdir(query, cwd, max);
    }

    // Try using `fd` for fast fuzzy search (respects .gitignore)
    if let Some(items) = list_files_via_fd(query, cwd, max)
        && !items.is_empty()
    {
        return items;
    }
    // Fallback: readdir
    list_files_via_readdir(query, cwd, max)
}

fn list_files_via_fd(query: &str, cwd: &Path, max: usize) -> Option<Vec<SelectItem>> {
    let base = bb_core::config::project_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    let is_project = base.join(".git").exists()
        || base.join("Cargo.toml").exists()
        || base.join("package.json").exists();

    let mut args = vec![
        "--base-directory".to_string(),
        base.to_string_lossy().to_string(),
        "--max-results".to_string(),
        max.to_string(),
        "--type".to_string(),
        "f".to_string(),
        "--type".to_string(),
        "d".to_string(),
        "--hidden".to_string(),
        "--exclude".to_string(),
        ".git".to_string(),
        "--exclude".to_string(),
        ".git/*".to_string(),
        "--exclude".to_string(),
        "node_modules".to_string(),
        "--exclude".to_string(),
        "target".to_string(),
        "--exclude".to_string(),
        "__pycache__".to_string(),
        "--exclude".to_string(),
        ".venv".to_string(),
    ];

    if query.is_empty() {
        // No query: limit depth unless we're in a proper project
        let depth = if is_project { "4" } else { "2" };
        args.extend(["--max-depth".to_string(), depth.to_string()]);
    } else if query.contains('/') {
        // Path-scoped query: search full path
        args.push("--full-path".to_string());
        args.push(query.to_string());
    } else {
        args.push(query.to_string());
    }

    let output = std::process::Command::new("fd").args(&args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<SelectItem> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .take(max)
        .map(|line| {
            let label = line.to_string();
            SelectItem {
                label: label.clone(),
                detail: None,
                value: line.to_string(),
            }
        })
        .collect();
    Some(items)
}

fn format_at_file_value(path: &str) -> String {
    if path.chars().any(char::is_whitespace) {
        let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
        format!("@\"{escaped}\"")
    } else {
        format!("@{path}")
    }
}

fn list_files_via_readdir(query: &str, cwd: &Path, max: usize) -> Vec<SelectItem> {
    let base = bb_core::config::project_root(cwd).unwrap_or_else(|| cwd.to_path_buf());
    // Determine which directory to list and what prefix to match
    let path_split = query.rsplit_once('/');
    let (dir, prefix) = if let Some((dir_part, file_part)) = path_split {
        (base.join(format!("{dir_part}/")), file_part.to_string())
    } else {
        (base, query.to_string())
    };

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let lower_prefix = prefix.to_lowercase();
    let mut items: Vec<SelectItem> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            // Skip hidden files unless query starts with .
            if name.starts_with('.') && !lower_prefix.starts_with('.') {
                return false;
            }
            lower_prefix.is_empty() || name.starts_with(&lower_prefix)
        })
        .take(max)
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let rel = if let Some((dir_part, _)) = path_split {
                format!("{dir_part}/{name}")
            } else {
                name.clone()
            };
            let display = if is_dir { format!("{name}/") } else { name };
            let raw_value = if is_dir {
                format!("{rel}/")
            } else {
                rel.clone()
            };
            SelectItem {
                label: display,
                detail: None,
                value: raw_value,
            }
        })
        .collect();

    // Directories first, then alphabetical
    items.sort_by(|a, b| {
        let a_dir = a.value.ends_with('/');
        let b_dir = b.value.ends_with('/');
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.label.cmp(&b.label),
        }
    });

    items
}

impl TuiState {
    pub(crate) fn render_tree_menu_lines(&self, width: usize) -> Option<Vec<String>> {
        self.tree_menu.as_ref().map(|menu| menu.render(width))
    }

    pub(crate) fn render_select_menu_lines(&self, width: usize) -> Option<Vec<String>> {
        self.select_menu.as_ref().map(|menu| menu.render(width))
    }

    pub(crate) fn render_slash_menu_lines(&self, width: usize) -> Option<Vec<String>> {
        self.slash_menu.as_ref().map(|menu| menu.render(width))
    }

    pub(crate) fn render_at_file_menu_lines(&self, width: usize) -> Option<Vec<String>> {
        self.at_file_menu.as_ref().map(|menu| menu.render(width))
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
            // Check for @ file menu instead
            self.update_at_file_menu();
            return;
        };
        self.at_file_menu = None; // slash menu takes precedence
        let extra = self.extra_slash_items.clone();
        let mut menu = self
            .slash_menu
            .take()
            .unwrap_or_else(|| TuiSlashMenuState::new(&extra));
        menu.set_search(&query);
        self.slash_menu = Some(menu);
    }

    pub(super) fn accept_slash_selection(&mut self, value: String) {
        self.input = value;
        self.cursor = self.input.len();
        self.slash_menu = None;
        self.dirty = true;
    }

    fn at_query_state(&self) -> Option<AtQueryState> {
        let before = self.input.get(..self.cursor)?;
        let at_pos = before.rfind('@')?;
        if at_pos > 0 {
            let prev = before.as_bytes().get(at_pos - 1)?;
            if !(*prev == b' ' || *prev == b'\t' || *prev == b'\n') {
                return None;
            }
        }

        let query = &before[at_pos + 1..];
        if let Some(rest) = query.strip_prefix('"') {
            if rest.contains('"') {
                return None;
            }
            return Some(AtQueryState {
                at_pos,
                query: rest.to_string(),
                quoted: true,
            });
        }

        if let Some(rest) = query.strip_prefix('\'') {
            if rest.contains('\'') {
                return None;
            }
            return Some(AtQueryState {
                at_pos,
                query: rest.to_string(),
                quoted: true,
            });
        }

        if query.contains(' ') {
            return None;
        }

        Some(AtQueryState {
            at_pos,
            query: query.to_string(),
            quoted: false,
        })
    }

    pub(super) fn update_at_file_menu(&mut self) {
        let Some(state) = self.at_query_state() else {
            self.at_file_menu = None;
            return;
        };
        let cwd = self.cwd.clone();
        if let Some(ref mut menu) = self.at_file_menu {
            menu.update(&state.query, &cwd);
        } else {
            self.at_file_menu = Some(AtFileMenuState::new(&state.query, &cwd));
        }
    }

    /// Accept the selected `@file` completion.
    pub(super) fn accept_at_file_selection(&mut self, value: String) {
        let Some(state) = self.at_query_state() else {
            self.at_file_menu = None;
            self.dirty = true;
            return;
        };

        let prefix = self.input[..state.at_pos].to_string();
        let suffix = self.input[self.cursor..].to_string();
        let is_dir = value.ends_with('/');
        let needs_quotes = state.quoted || value.chars().any(char::is_whitespace);
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        let inserted = if needs_quotes {
            if is_dir {
                format!("@\"{escaped}")
            } else {
                format!("@\"{escaped}\"")
            }
        } else {
            format_at_file_value(&value)
        };
        let trailing = if is_dir { "" } else { " " };

        self.input = format!("{prefix}{inserted}{trailing}{suffix}");
        self.cursor = prefix.len() + inserted.len() + trailing.len();

        if is_dir {
            self.update_at_file_menu();
        } else {
            self.at_file_menu = None;
        }
        self.dirty = true;
    }
}
