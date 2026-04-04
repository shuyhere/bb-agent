#![allow(dead_code)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::runtime::FullscreenState;
use super::transcript::BlockId;
use super::types::FullscreenMode;

impl FullscreenState {
    pub(super) fn on_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc if key.modifiers == KeyModifiers::NONE => {
                self.mode = FullscreenMode::Transcript;
                self.status_line = self.mode_help_text();
                self.dirty = true;
            }
            KeyCode::Enter if key.modifiers == KeyModifiers::NONE => {
                self.mode = FullscreenMode::Transcript;
                if self.search.query.trim().is_empty() {
                    self.status_line =
                        "search scaffold ready • type after / to filter transcript".to_string();
                    self.dirty = true;
                } else {
                    self.search_step(true);
                }
            }
            KeyCode::Backspace if key.modifiers == KeyModifiers::NONE => {
                self.search.query.pop();
                self.status_line = format!("search {}", self.search_prompt());
                self.dirty = true;
            }
            KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search.query.push(ch);
                self.status_line = format!("search {}", self.search_prompt());
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub(super) fn search_prompt(&self) -> String {
        if self.search.query.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", self.search.query)
        }
    }

    pub(crate) fn search_step(&mut self, forward: bool) {
        let query = self.search.query.trim().to_ascii_lowercase();
        if query.is_empty() {
            self.status_line =
                "search scaffold ready • press / and type to jump between transcript blocks"
                    .to_string();
            self.dirty = true;
            return;
        }

        let blocks = self.focusable_blocks();
        if blocks.is_empty() {
            return;
        }

        let current = self
            .focused_block
            .and_then(|block_id| blocks.iter().position(|candidate| *candidate == block_id))
            .unwrap_or(0);

        for offset in 1..=blocks.len() {
            let index = if forward {
                (current + offset) % blocks.len()
            } else {
                (current + blocks.len() - (offset % blocks.len())) % blocks.len()
            };
            let block_id = blocks[index];
            if self.block_matches_query(block_id, &query) {
                self.focus_block(block_id);
                self.status_line = format!("matched {}", self.search_prompt());
                return;
            }
        }

        self.status_line = format!("no matches for {}", self.search_prompt());
        self.dirty = true;
    }

    fn block_matches_query(&self, block_id: BlockId, query: &str) -> bool {
        let Some(block) = self.transcript.block(block_id) else {
            return false;
        };
        format!("{}\n{}", block.title, block.content)
            .to_ascii_lowercase()
            .contains(query)
    }
}
