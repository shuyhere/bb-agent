use super::*;

impl InteractiveMode {
    pub(super) fn editor_text(&self) -> String {
        self.editor
            .lock()
            .map(|e| e.get_text())
            .unwrap_or_default()
    }

    pub(super) fn set_editor_text(&mut self, text: &str) {
        if let Ok(mut e) = self.editor.lock() {
            e.set_text(text);
        }
        self.sync_bash_mode_from_editor();
    }

    pub(super) fn clear_editor(&mut self) {
        if let Ok(mut e) = self.editor.lock() {
            e.clear();
        }
        self.sync_bash_mode_from_editor();
    }

    pub(super) fn push_editor_history(&mut self, text: &str) {
        if let Ok(mut e) = self.editor.lock() {
            e.add_to_history(text);
        }
    }

    pub(super) fn set_bash_mode(&mut self, value: bool) {
        if let Ok(mut bash_mode) = self.is_bash_mode.lock() {
            *bash_mode = value;
        }
    }

    pub(super) fn sync_bash_mode_from_editor(&mut self) {
        let is_bash_mode = self.editor_text().trim_start().starts_with('!');
        self.set_bash_mode(is_bash_mode);
    }

    pub(super) fn start_background_checks(&mut self) {
        // Background checks are deferred - no TODO noise in the UI
    }

    pub(super) fn get_changelog_for_display(&self) -> Option<String> {
        None
    }

    pub(super) async fn bind_current_session_extensions(&mut self) -> InteractiveResult<()> {
        // Extension binding is deferred
        Ok(())
    }

    pub(super) fn render_initial_messages(&mut self) {
        // No startup noise - pi doesn't show "initialized" messages
    }

    pub(super) fn update_terminal_title(&mut self) {
        let cwd = self
            .controller
            .runtime_host
            .cwd()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("BB-Agent");
        self.ui
            .terminal
            .write(&format!("\x1b]0;BB-Agent interactive - {cwd}\x07"));
    }

    pub(super) fn stop_ui(&mut self) {
        self.ui.stop();
    }
}
