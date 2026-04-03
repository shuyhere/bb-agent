use crate::slash_commands::matches_shared_local_slash_submission;

use super::{
    runtime::FullscreenState,
    transcript::{BlockKind, NewBlock},
    types::FullscreenSubmission,
};

impl FullscreenState {
    pub(super) fn submit_input(&mut self) {
        let submitted = self.input.trim_end().to_string();
        if submitted.trim().is_empty() {
            self.status_line = "empty input ignored".to_string();
            self.dirty = true;
            return;
        }

        if matches_shared_local_slash_submission(&submitted) {
            self.submit_local_command(submitted);
            return;
        }

        self.transcript.append_root_block(
            NewBlock::new(BlockKind::UserMessage, "prompt").with_content(submitted.clone()),
        );
        self.submitted_inputs.push(submitted.clone());
        self.pending_submissions
            .push_back(FullscreenSubmission::Input(submitted));
        self.input.clear();
        self.cursor = 0;
        self.slash_menu = None;
        self.select_menu = None;
        self.status_line = "Working...".to_string();
        self.projection_dirty = true;
        self.dirty = true;
    }

    pub(super) fn submit_local_command(&mut self, submitted: String) {
        self.pending_submissions
            .push_back(FullscreenSubmission::Input(submitted));
        self.input.clear();
        self.cursor = 0;
        self.slash_menu = None;
        self.select_menu = None;
        self.status_line = self.mode_help_text();
        self.dirty = true;
    }

    pub(super) fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.update_slash_menu();
        self.dirty = true;
    }

    pub(super) fn insert_str(&mut self, text: &str) {
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.update_slash_menu();
        self.dirty = true;
    }

    pub(super) fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = previous_boundary(&self.input, self.cursor);
        self.input.drain(prev..self.cursor);
        self.cursor = prev;
        self.update_slash_menu();
        self.dirty = true;
    }

    pub(super) fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor = previous_boundary(&self.input, self.cursor);
        self.update_slash_menu();
        self.dirty = true;
    }

    pub(super) fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        self.cursor = next_boundary(&self.input, self.cursor);
        self.update_slash_menu();
        self.dirty = true;
    }
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .char_indices()
        .nth(1)
        .map(|(idx, _)| cursor + idx)
        .unwrap_or(text.len())
}
