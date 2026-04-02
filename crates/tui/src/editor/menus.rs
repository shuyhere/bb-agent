use super::types::Editor;
use crate::fuzzy::fuzzy_filter;
use crate::select_list::{SelectItem, SelectList};

impl Editor {
    pub(super) fn slash_query(&self) -> Option<String> {
        if self.state.cursor_line != 0 {
            return None;
        }
        let line = &self.state.lines[0];
        let before = &line[..self.state.cursor_col.min(line.len())];
        if !before.starts_with('/') {
            return None;
        }
        if before.contains(' ') || before.contains('\n') {
            return None;
        }
        Some(before.to_string())
    }

    pub(super) fn update_slash_menu(&mut self) {
        let Some(query) = self.slash_query() else {
            self.slash_menu = None;
            return;
        };
        let mut list = SelectList::new(self.slash_commands.clone(), 6);
        list.set_show_search(false);
        let search = query.trim_start_matches('/');
        list.set_search(search);
        self.slash_menu = Some(list);
    }

    pub(super) fn accept_slash_selection(&mut self, value: String) {
        self.state.lines[0] = value;
        self.state.cursor_line = 0;
        self.state.cursor_col = self.state.lines[0].len();
        self.slash_menu = None;
    }

    /// Detect `@query` before the cursor. Returns the full `@...` token if found.
    pub(super) fn file_query(&self) -> Option<String> {
        let line = &self.state.lines[self.state.cursor_line];
        let before = &line[..self.state.cursor_col.min(line.len())];
        // Walk backwards to find the `@`
        let mut at_pos = None;
        for (i, c) in before.char_indices().rev() {
            if c == '@' {
                // Make sure it's at start of line or preceded by whitespace
                if i == 0 || before[..i].ends_with(|ch: char| ch.is_whitespace()) {
                    at_pos = Some(i);
                }
                break;
            }
            // If we hit whitespace before finding @, no match
            if c.is_whitespace() {
                return None;
            }
        }
        at_pos.map(|pos| before[pos..].to_string())
    }

    /// Recursively scan a directory for files up to max_depth.
    pub(super) fn scan_files(
        dir: &std::path::Path,
        base: &std::path::Path,
        depth: usize,
        max_depth: usize,
        results: &mut Vec<String>,
    ) {
        if depth > max_depth {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip hidden files/dirs
            if name.starts_with('.') {
                continue;
            }
            // Skip common noisy directories
            if path.is_dir()
                && matches!(
                    name.as_str(),
                    "node_modules" | "target" | "dist" | "build" | ".git" | "__pycache__"
                )
            {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(base) {
                let rel_str = rel.to_string_lossy().to_string();
                results.push(rel_str);
            }
            if path.is_dir() {
                Self::scan_files(&path, base, depth + 1, max_depth, results);
            }
        }
    }

    pub(super) fn update_file_menu(&mut self) {
        let Some(query) = self.file_query() else {
            self.file_menu = None;
            return;
        };
        let search = query.trim_start_matches('@');
        // Scan files
        let mut files = Vec::new();
        Self::scan_files(&self.cwd, &self.cwd, 0, 3, &mut files);
        files.sort();

        // Filter using fuzzy_filter
        let filtered = fuzzy_filter(files, search, |f| f.as_str());

        // Build SelectItems from filtered results (cap at 100)
        let items: Vec<SelectItem> = filtered
            .into_iter()
            .take(100)
            .map(|f| SelectItem {
                label: f.clone(),
                detail: None,
                value: f,
            })
            .collect();

        if items.is_empty() {
            self.file_menu = None;
            return;
        }

        let mut list = SelectList::new(items, 8);
        list.set_show_search(false);
        self.file_menu = Some(list);
    }

    pub(super) fn accept_file_selection(&mut self, path: String) {
        let Some(query) = self.file_query() else {
            self.file_menu = None;
            return;
        };
        let line = &self.state.lines[self.state.cursor_line];
        let before = &line[..self.state.cursor_col.min(line.len())];
        // Find the start of the @query token
        let at_start = before.len() - query.len();
        let replacement = format!("@{}", path);
        let new_line = format!(
            "{}{}{}",
            &line[..at_start],
            replacement,
            &line[self.state.cursor_col..]
        );
        self.state.lines[self.state.cursor_line] = new_line;
        self.state.cursor_col = at_start + replacement.len();
        self.file_menu = None;
    }
}
