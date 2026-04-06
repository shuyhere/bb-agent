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

        // Expand paste markers back to full content before submitting.
        let expanded = self.expand_paste_markers(&submitted);

        // Show the collapsed version in transcript (keeps it readable)
        self.transcript.append_root_block(
            NewBlock::new(BlockKind::UserMessage, "prompt").with_content(submitted.clone()),
        );
        self.submitted_inputs.push(submitted.clone());

        // Include any pending image attachments with this submission.
        let image_paths = self.take_pending_image_paths();
        if image_paths.is_empty() {
            self.pending_submissions
                .push_back(FullscreenSubmission::Input(expanded));
        } else {
            self.pending_submissions
                .push_back(FullscreenSubmission::InputWithImages {
                    text: expanded,
                    image_paths,
                });
        }

        self.input.clear();
        self.cursor = 0;
        self.slash_menu = None;
        self.select_menu = None;
        self.at_file_menu = None;
        self.status_line = "Working...".to_string();
        // Clear paste storage after submit
        self.paste_storage.clear();
        self.paste_counter = 0;
        self.projection_dirty = true;
        self.dirty = true;
    }

    /// Expand `[paste #N ...]` markers back to their stored content.
    fn expand_paste_markers(&self, text: &str) -> String {
        if self.paste_storage.is_empty() {
            return text.to_string();
        }
        let mut result = text.to_string();
        for (&id, content) in &self.paste_storage {
            // Match markers like [paste #1 +123 lines] or [paste #1 1234 chars]
            let patterns = [format!("[paste #{id} "), format!("[paste #{id}]")];
            for pat in &patterns {
                if let Some(start) = result.find(pat.as_str()) {
                    // Find the closing bracket
                    if let Some(end) = result[start..].find(']') {
                        result.replace_range(start..start + end + 1, content);
                        break; // Only replace first occurrence per ID
                    }
                }
            }
        }
        result
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
        self.update_at_file_menu();
        self.dirty = true;
    }

    pub(super) fn insert_str(&mut self, text: &str) {
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.update_slash_menu();
        self.update_at_file_menu();
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
        self.update_at_file_menu();
        self.dirty = true;
    }

    pub(super) fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor = previous_boundary(&self.input, self.cursor);
        self.update_slash_menu();
        self.update_at_file_menu();
        self.dirty = true;
    }

    pub(super) fn move_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        self.cursor = next_boundary(&self.input, self.cursor);
        self.update_slash_menu();
        self.update_at_file_menu();
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
