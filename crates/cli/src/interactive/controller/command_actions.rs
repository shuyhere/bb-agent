use super::*;

impl InteractiveMode {
    pub(super) fn handle_export_command(&mut self, text: &str) {
        self.show_status(format!("TODO: export command {text}"));
    }

    pub(super) fn handle_import_command(&mut self, text: &str) {
        self.show_status(format!("TODO: import command {text}"));
    }

    pub(super) fn handle_share_command(&mut self) {
        self.show_status("TODO: share command");
    }

    pub(super) fn handle_copy_command(&mut self) {
        self.show_status("TODO: copy command");
    }

    pub(super) fn handle_name_command(&mut self, text: &str) {
        let name = text.strip_prefix("/name").unwrap_or(text).trim();
        if name.is_empty() {
            self.show_status("Usage: /name <session name>");
            return;
        }
        match self.session_setup.conn.execute(
            "UPDATE sessions SET name = ?1, updated_at = datetime('now') WHERE session_id = ?2",
            params![name, self.session_setup.session_id],
        ) {
            Ok(_) => self.show_status(format!("Session renamed to: {name}")),
            Err(e) => self.show_status(format!("Failed to rename session: {e}")),
        }
    }

    pub(super) fn handle_session_command(&mut self) {
        let bold = "\x1b[1m";
        let dim = "\x1b[2m";
        let reset = "\x1b[0m";

        let session_id = &self.session_setup.session_id;
        let session_row = store::get_session(&self.session_setup.conn, session_id)
            .ok()
            .flatten();
        let session_name = session_row.as_ref().and_then(|r| r.name.as_deref());
        let session_file = self.session_setup.conn
            .path()
            .map(|p| p.to_string())
            .unwrap_or_else(|| "in-memory".into());

        // Count messages by role from session entries
        let mut user_msgs = 0_u64;
        let mut asst_msgs = 0_u64;
        let mut tool_calls = 0_u64;
        let mut tool_results = 0_u64;
        let mut total_input = 0_u64;
        let mut total_output = 0_u64;
        let mut total_cache_read = 0_u64;
        let mut total_cache_write = 0_u64;
        let mut total_cost = 0.0_f64;

        if let Ok(rows) = store::get_entries(&self.session_setup.conn, session_id) {
            for row in rows {
                if let Ok(entry) = store::parse_entry(&row) {
                    match &entry {
                        bb_core::types::SessionEntry::Message { message, .. } => match message {
                            bb_core::types::AgentMessage::User(_) => user_msgs += 1,
                            bb_core::types::AgentMessage::Assistant(a) => {
                                asst_msgs += 1;
                                tool_calls += a.content.iter().filter(|c| {
                                    matches!(c, bb_core::types::AssistantContent::ToolCall { .. })
                                }).count() as u64;
                                total_input += a.usage.input;
                                total_output += a.usage.output;
                                total_cache_read += a.usage.cache_read;
                                total_cache_write += a.usage.cache_write;
                                total_cost += a.usage.cost.total;
                            }
                            bb_core::types::AgentMessage::ToolResult(_) => tool_results += 1,
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }

        let total_msgs = user_msgs + asst_msgs + tool_results;
        let total_tokens = total_input + total_output + total_cache_read + total_cache_write;

        let mut info = format!("{bold}Session Info{reset}\n\n");
        if let Some(name) = session_name {
            info.push_str(&format!("{dim}Name:{reset} {name}\n"));
        }
        info.push_str(&format!("{dim}File:{reset} {session_file}\n"));
        info.push_str(&format!("{dim}ID:{reset} {session_id}\n\n"));

        info.push_str(&format!("{bold}Messages{reset}\n"));
        info.push_str(&format!("{dim}User:{reset} {user_msgs}\n"));
        info.push_str(&format!("{dim}Assistant:{reset} {asst_msgs}\n"));
        info.push_str(&format!("{dim}Tool Calls:{reset} {tool_calls}\n"));
        info.push_str(&format!("{dim}Tool Results:{reset} {tool_results}\n"));
        info.push_str(&format!("{dim}Total:{reset} {total_msgs}\n\n"));

        info.push_str(&format!("{bold}Tokens{reset}\n"));
        info.push_str(&format!("{dim}Input:{reset} {total_input}\n"));
        info.push_str(&format!("{dim}Output:{reset} {total_output}\n"));
        if total_cache_read > 0 {
            info.push_str(&format!("{dim}Cache Read:{reset} {total_cache_read}\n"));
        }
        if total_cache_write > 0 {
            info.push_str(&format!("{dim}Cache Write:{reset} {total_cache_write}\n"));
        }
        info.push_str(&format!("{dim}Total:{reset} {total_tokens}\n"));

        if total_cost > 0.0 {
            info.push_str(&format!("\n{bold}Cost{reset}\n"));
            info.push_str(&format!("{dim}Total:{reset} ${total_cost:.4}"));
        }

        self.render_state_mut()
            .add_message_to_chat(super::super::events::InteractiveMessage::System {
                text: info,
            });
        self.rebuild_chat_container();
        self.refresh_ui();
    }

    pub(super) fn handle_changelog_command(&mut self) {
        self.show_status("TODO: changelog command");
    }

    pub(super) fn handle_hotkeys_command(&mut self) {
        let hotkeys = vec![
            "Key Bindings:",
            "  Ctrl+C      - Interrupt / clear input",
            "  Ctrl+D      - Exit (on empty input)",
            "  Ctrl+Z      - Suspend",
            "  Ctrl+J      - Cycle thinking level",
            "  Ctrl+K      - Cycle model forward",
            "  Ctrl+L      - Toggle tool output expansion",
            "  Ctrl+T      - Toggle thinking visibility",
            "  Ctrl+E      - Open external editor",
            "  Ctrl+R      - Resume session selector",
            "  Ctrl+N      - New session",
            "  Ctrl+F      - Follow-up message",
            "  Ctrl+V      - Paste image from clipboard",
            "  Esc         - Cancel / back",
        ];
        for line in hotkeys {
            self.render_cache.chat_lines.push(line.to_string());
        }
    }

    pub(super) fn show_user_message_selector(&mut self) {
        let _ = self.controller.commands.show_user_message_selector();
        self.show_placeholder("user message selector");
    }

    pub(super) fn show_tree_selector(&mut self) {
        let _ = self.controller.commands.open_placeholder_selector(
            SelectorKind::Tree,
            "Session Tree",
        );
        self.show_placeholder("session tree selector");
    }

    pub(super) fn handle_clear_command(&mut self) {
        let _ = self.controller.runtime_host.session_mut().clear_queue();
        self.render_cache.chat_lines.clear();
        self.render_cache.pending_lines.clear();
        self.queues.steering_queue.clear();
        self.queues.follow_up_queue.clear();
        self.queues.compaction_queued_messages.clear();
        self.render_state_mut().chat_items.clear();
        self.render_state_mut().pending_items.clear();
        self.show_status("Started a fresh interactive session shell around the core session");
    }

    pub(super) fn handle_new_session(&mut self) {
        let cwd_str = self.session_setup.tool_ctx.cwd.display().to_string();
        match store::create_session(&self.session_setup.conn, &cwd_str) {
            Ok(new_id) => {
                self.session_setup.session_id = new_id.clone();
                self.options.session_id = Some(new_id.clone());
                let _ = self.controller.runtime_host.session_mut().clear_queue();

                // Clear all chat/pending/streaming state (match pi's renderCurrentSessionState)
                self.render_state_mut().chat_items.clear();
                self.render_state_mut().pending_items.clear();
                self.render_state_mut().streaming_component = None;
                self.streaming.streaming_text.clear();
                self.streaming.streaming_thinking.clear();
                self.streaming.streaming_tool_calls.clear();
                self.streaming.is_streaming = false;
                self.queues.steering_queue.clear();
                self.queues.follow_up_queue.clear();
                self.queues.compaction_queued_messages.clear();
                self.queues.pending_bash_components.clear();

                // Rebuild containers from scratch so TUI matches cleared state
                self.rebuild_chat_container();
                self.rebuild_pending_container();
                self.rebuild_footer();

                // Show confirmation (like pi's "New session started")
                self.render_state_mut()
                    .add_message_to_chat(super::super::events::InteractiveMessage::System {
                        text: "New session started".to_string(),
                    });
                self.rebuild_chat_container();
                self.refresh_ui();
            }
            Err(e) => {
                self.show_warning(format!("Failed to create new session: {e}"));
            }
        }
    }

    pub(super) fn handle_help_command(&mut self) {
        let commands = vec![
            "Available commands:",
            "  /help        - Show this help message",
            "  /new         - Create a new session",
            "  /name <name> - Rename current session",
            "  /session     - Show session info",
            "  /compact     - Trigger conversation compaction",
            "  /clear       - Clear chat display",
            "  /model       - Switch model",
            "  /hotkeys     - Show key bindings",
            "  /export      - Export session",
            "  /import      - Import session",
            "  /share       - Share session",
            "  /copy        - Copy last response",
            "  /debug       - Show debug info",
            "  /reload      - Reload resources",
            "  /quit        - Exit the application",
            "  !<cmd>       - Execute bash command",
            "  !!<cmd>      - Execute bash (excluded from context)",
        ];
        for line in commands {
            self.render_cache.chat_lines.push(line.to_string());
        }
    }

    pub(super) fn check_auto_compaction(&mut self) {
        let session_id = self.session_setup.session_id.clone();
        let settings = bb_core::types::CompactionSettings::default();
        if let Ok(entries) = store::get_entries(&self.session_setup.conn, &session_id) {
            let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
            let window = self.session_setup.model.context_window;
            if compaction::should_compact(total_tokens, window, &settings) {
                self.render_cache.chat_lines.push(format!(
                    "[c] Auto-compaction triggered ({total_tokens} tokens, window {window})"
                ));
                // Prepare and note - full async LLM summarization deferred to future wave
                if let Some(prep) = compaction::prepare_compaction(&entries, &settings) {
                    self.render_cache.chat_lines.push(format!(
                        "[c] {} messages to summarize, {} kept",
                        prep.messages_to_summarize.len(),
                        prep.kept_messages.len()
                    ));
                }
            }
        }
    }

    pub(super) fn handle_compact_command(&mut self, instructions: Option<&str>) {
        self.interaction.is_compacting = true;
        let session_id = self.session_setup.session_id.clone();
        match store::get_entries(&self.session_setup.conn, &session_id) {
            Ok(entries) => {
                let settings = bb_core::types::CompactionSettings::default();
                let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
                match compaction::prepare_compaction(&entries, &settings) {
                    Some(prep) => {
                        let to_summarize = prep.messages_to_summarize.len();
                        let kept = prep.kept_messages.len();
                        self.render_cache.chat_lines.push(format!(
                            "Compaction: {total_tokens} estimated tokens, {to_summarize} messages to summarize, {kept} kept"
                        ));
                        if let Some(inst) = instructions {
                            self.render_cache.chat_lines.push(format!("Instructions: {inst}"));
                        }
                        self.show_status("Compaction prepared (async LLM summarization not wired in interactive mode yet)");
                    }
                    None => {
                        self.show_status(format!("Nothing to compact ({total_tokens} estimated tokens, {} entries)", entries.len()));
                    }
                }
            }
            Err(e) => {
                self.show_status(format!("Failed to get entries for compaction: {e}"));
            }
        }
        self.interaction.is_compacting = false;
    }

    pub(super) fn handle_reload_command(&mut self) {
        self.show_status("TODO: reload resources/extensions");
    }

    pub(super) fn handle_debug_command(&mut self) {
        self.show_status("TODO: debug command");
    }

    pub(super) fn handle_armin_says_hi(&mut self) {
        self.render_state_mut()
            .add_message_to_chat(InteractiveMessage::Assistant {
                message: assistant_message_from_parts("hi armin 👋", None, false),
                tool_calls: Vec::new(),
            });
    }

    pub(super) fn show_session_selector(&mut self) {
        let _ = self.controller.commands.open_placeholder_selector(
            SelectorKind::Session,
            "Session Selector",
        );
        self.show_placeholder("session selector");
    }

    pub(super) fn shutdown(&mut self) {
        self.interaction.shutdown_requested = true;
        self.show_status("Shutdown requested");
    }

    pub(super) fn handle_bash_command(&mut self, command: &str, excluded_from_context: bool) {
        let label = if excluded_from_context { "bash(excluded)" } else { "bash" };
        self.render_cache.chat_lines.push(format!("{label}> {command}"));
        let output = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .current_dir(&self.session_setup.tool_ctx.cwd)
            .output();
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if !stdout.is_empty() {
                    for line in stdout.lines() {
                        self.render_cache.chat_lines.push(line.to_string());
                    }
                }
                if !stderr.is_empty() {
                    for line in stderr.lines() {
                        self.render_cache.chat_lines.push(format!("stderr: {line}"));
                    }
                }
                if !out.status.success() {
                    self.render_cache.chat_lines.push(format!("exit code: {}", out.status.code().unwrap_or(-1)));
                }
            }
            Err(e) => {
                self.render_cache.chat_lines.push(format!("Failed to execute command: {e}"));
            }
        }
    }

    pub(super) fn flush_pending_bash_components(&mut self) {
        while let Some(line) = self.queues.pending_bash_components.pop_front() {
            self.render_cache.chat_lines.push(line);
        }
    }

    pub(super) fn is_extension_command(&self, text: &str) -> bool {
        text.starts_with("/ext") || text.starts_with("/extension")
    }

    pub(super) fn queue_compaction_message(&mut self, text: String, kind: QueuedMessageKind) {
        self.queues.compaction_queued_messages
            .push_back(QueuedMessage { text, kind });
        self.show_status("Queued message while compaction is active");
    }

    pub(super) fn show_model_selector(&mut self, initial_search: Option<&str>) {
        let current_model = self
            .controller
            .runtime_host
            .session()
            .model()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .unwrap_or_else(|| format!("{}/{}", self.session_setup.model.provider, self.session_setup.model.id));

        let mut models = self.get_model_candidates();
        let current_provider = self.session_setup.model.provider.clone();
        let current_id = self.session_setup.model.id.clone();
        models.sort_by(|a, b| {
            let a_current = a.provider == current_provider && a.id == current_id;
            let b_current = b.provider == current_provider && b.id == current_id;
            b_current
                .cmp(&a_current)
                .then_with(|| a.provider.cmp(&b.provider))
                .then_with(|| a.id.cmp(&b.id))
        });

        let mut selector = ModelSelector::from_models(models, 10);
        if let Some(query) = initial_search.filter(|s| !s.is_empty()) {
            selector.set_search(query);
        }
        let component = Box::new(ModelSelectorOverlay::new(
            selector,
            current_model,
            initial_search.map(|s| s.to_string()),
        ));
        self.ui.tui.show_overlay(component);
        self.show_status("Opened model selector");
    }

    pub(super) fn show_placeholder(&mut self, label: &str) {
        self.show_status(format!("TODO: {label}"));
    }
}
