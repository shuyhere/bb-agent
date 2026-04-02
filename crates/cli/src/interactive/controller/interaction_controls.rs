use super::*;

impl InteractiveMode {
    pub(super) fn handle_escape(&mut self) {
        // Priority 1: dismiss overlay
        if self.ui.tui.has_overlay() {
            self.ui.tui.hide_overlay();
            return;
        }
        // Priority 2: abort loading animation
        if self.streaming.status_loader.is_some() {
            self.streaming.status_loader = None;
            self.show_status("Aborted loading");
            return;
        }
        // Priority 3: cancel bash run
        if self.interaction.is_bash_running {
            self.interaction.is_bash_running = false;
            self.show_warning("Canceled bash placeholder run");
            return;
        }
        // Priority 4: exit bash mode
        if self
            .interaction.is_bash_mode
            .lock()
            .map(|value| *value)
            .unwrap_or(false)
        {
            self.clear_editor();
            self.set_bash_mode(false);
            self.show_status("Exited bash mode");
            return;
        }
        // Priority 5: abort streaming
        if self.streaming.is_streaming {
            self.streaming.is_streaming = false;
            self.show_warning("Aborted");
            return;
        }
        // Priority 6: clear editor if it has text
        if !self.editor_text().trim().is_empty() {
            self.clear_editor();
            self.show_status("Editor cleared");
            return;
        }
        // Priority 7: double-escape -> tree selector
        let now = Instant::now();
        let activate = self
            .interaction.last_escape_time
            .map(|last| now.saturating_duration_since(last) < Duration::from_millis(500))
            .unwrap_or(false);
        if activate {
            self.show_tree_selector();
            self.interaction.last_escape_time = None;
        } else {
            self.interaction.last_escape_time = Some(now);
        }
    }

    pub(super) fn handle_ctrl_c(&mut self) {
        // If streaming, abort and show "Aborted"
        if self.streaming.is_streaming {
            self.streaming.is_streaming = false;
            self.show_warning("Aborted");
            self.interaction.last_sigint_time = Some(Instant::now());
            return;
        }
        // If editor has text, clear it
        if !self.editor_text().trim().is_empty() {
            self.clear_editor();
            self.show_status("Editor cleared");
            self.interaction.last_sigint_time = Some(Instant::now());
            return;
        }
        // Double Ctrl-C -> shutdown
        let now = Instant::now();
        let is_double = self
            .interaction.last_sigint_time
            .map(|last| now.saturating_duration_since(last) < Duration::from_millis(500))
            .unwrap_or(false);
        self.interaction.last_sigint_time = Some(now);

        if is_double {
            self.interaction.shutdown_requested = true;
            self.show_warning("Exiting interactive mode");
        } else {
            self.show_status("Interrupt requested. Press Ctrl-C again to exit.");
        }
    }

    pub(super) fn handle_ctrl_d(&mut self) {
        if self.editor_text().trim().is_empty() {
            self.interaction.shutdown_requested = true;
            self.show_status("EOF received on empty editor; shutting down");
        }
    }

    pub(super) fn handle_ctrl_z(&mut self) {
        self.show_warning("Suspend is not wired in the non-fullscreen skeleton yet");
    }

    pub(super) fn cycle_thinking_level(&mut self) {
        let current = self.controller.runtime_host.session().thinking_level();
        let next = match current {
            ThinkingLevel::Off => ThinkingLevel::Low,
            ThinkingLevel::Low => ThinkingLevel::Medium,
            ThinkingLevel::Medium => ThinkingLevel::High,
            ThinkingLevel::High | ThinkingLevel::XHigh => ThinkingLevel::Off,
        };
        self.controller
            .runtime_host
            .session_mut()
            .set_thinking_level(next);
        let label = match next {
            ThinkingLevel::Off => "off",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::XHigh => "xhigh",
        };
        self.show_status(format!("Thinking level: {label}"));
        self.rebuild_footer();
    }

    pub(super) fn cycle_model(&mut self, direction: &str) {
        let mut models = self.get_model_candidates();
        if models.is_empty() {
            self.show_warning("No models available");
            return;
        }
        models.sort_by(|a, b| {
            a.provider
                .cmp(&b.provider)
                .then_with(|| a.id.cmp(&b.id))
        });

        let current_provider = self.session_setup.model.provider.clone();
        let current_id = self.session_setup.model.id.clone();
        let current_idx = models
            .iter()
            .position(|m| m.provider == current_provider && m.id == current_id)
            .unwrap_or(0);
        let next_idx = match direction {
            "backward" => {
                if current_idx == 0 { models.len() - 1 } else { current_idx - 1 }
            }
            _ => (current_idx + 1) % models.len(),
        };
        if let Some(model) = models.get(next_idx).cloned() {
            self.apply_model_selection(model);
        }
    }

    pub(super) fn toggle_tool_output_expansion(&mut self) {
        self.interaction.tool_output_expanded = !self.interaction.tool_output_expanded;
        let state_label = if self.interaction.tool_output_expanded {
            "enabled"
        } else {
            "collapsed"
        };
        self.show_status(format!("tool output expansion {state_label}"));
        // Re-render chat to reflect new expansion state
        self.rebuild_chat_container();
        self.rebuild_pending_container();
    }

    pub(super) fn toggle_thinking_block_visibility(&mut self) {
        self.streaming.hide_thinking_block = !self.streaming.hide_thinking_block;
        let state_label = if self.streaming.hide_thinking_block {
            "hidden"
        } else {
            "expanded"
        };
        self.show_status(format!("thinking block {state_label}"));

        let hide_thinking_block = self.streaming.hide_thinking_block;
        let hidden_thinking_label = self.streaming.hidden_thinking_label.clone();
        for item in &mut self.render_state_mut().chat_items {
            if let ChatItem::AssistantMessage(component) = item {
                component.set_hide_thinking_block(hide_thinking_block);
                component.set_hidden_thinking_label(hidden_thinking_label.clone());
            }
        }
        if let Some(component) = self.render_state_mut().streaming_component.as_mut() {
            component.set_hide_thinking_block(hide_thinking_block);
            component.set_hidden_thinking_label(hidden_thinking_label);
        }

        self.rebuild_chat_container();
        self.rebuild_pending_container();
    }

    pub(super) fn handle_follow_up(&mut self) {
        let text = self.editor_text().trim().to_string();
        if text.is_empty() {
            self.show_status("Nothing to queue as follow-up");
            return;
        }
        self.push_editor_history(&text);
        self.clear_editor();
        self.queues.follow_up_queue.push_back(text);
        self.sync_pending_render_state();
        self.show_status("Queued follow-up message");
    }

    pub(super) fn handle_dequeue(&mut self) {
        // Pop from follow-up queue first, then steering queue
        let popped = if let Some(text) = self.queues.follow_up_queue.pop_back() {
            Some(text)
        } else {
            self.queues.steering_queue.pop_back()
        };
        if let Some(text) = popped {
            let current = self.editor_text();
            if current.trim().is_empty() {
                self.set_editor_text(&text);
            } else {
                self.set_editor_text(&format!("{text}\n\n{current}"));
            }
            self.sync_pending_render_state();
            self.show_status("Restored queued message to editor");
        } else {
            self.show_status("No queued messages to restore");
        }
    }

    pub(super) fn handle_clipboard_image_paste(&mut self) {
        self.show_status("TODO: clipboard image paste");
    }
}
