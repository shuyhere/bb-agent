use super::*;

impl InteractiveMode {
    pub(super) fn make_streaming_assistant_message(
        &self,
        stop_reason: Option<components::assistant_message::AssistantStopReason>,
        error_message: Option<String>,
    ) -> components::assistant_message::AssistantMessage {
        let mut message = assistant_message_from_parts(
            &self.streaming_text,
            if self.streaming_thinking.is_empty() {
                None
            } else {
                Some(self.streaming_thinking.clone())
            },
            !self.streaming_tool_calls.is_empty(),
        );
        message.stop_reason = stop_reason.or(Some(components::assistant_message::AssistantStopReason::Other));
        message.error_message = error_message;
        message
    }

    pub(super) fn sync_streaming_assistant_component(
        &mut self,
        stop_reason: Option<components::assistant_message::AssistantStopReason>,
        error_message: Option<String>,
    ) {
        let message = self.make_streaming_assistant_message(stop_reason, error_message);
        if self.render_state().streaming_component.is_none() {
            let hide = self.hide_thinking_block;
            let label = self.hidden_thinking_label.clone();
            let mut comp = components::assistant_message::AssistantMessageComponent::new(
                Some(message.clone()),
                hide,
            );
            comp.set_hidden_thinking_label(label);
            self.render_state_mut().streaming_component = Some(comp.clone());
            self.render_state_mut().streaming_message = Some(message);
            self.render_state_mut().chat_items.push(ChatItem::AssistantMessage(comp));
        } else {
            if let Some(comp) = self.render_state_mut().streaming_component.as_mut() {
                comp.update_content(message.clone());
            }
            self.render_state_mut().streaming_message = Some(message);
            let updated = self.render_state().streaming_component.clone();
            if let Some(updated) = updated {
                if let Some(item) = self
                    .render_state_mut()
                    .chat_items
                    .iter_mut()
                    .rev()
                    .find(|i| matches!(i, ChatItem::AssistantMessage(_)))
                {
                    *item = ChatItem::AssistantMessage(updated);
                }
            }
        }
    }

    pub(super) fn update_streaming_display(&mut self) {
        self.sync_streaming_assistant_component(None, None);
        self.refresh_ui();
    }

    pub(super) fn finalize_streaming_assistant_message(
        &mut self,
        stop_reason: Option<components::assistant_message::AssistantStopReason>,
        error_message: Option<String>,
    ) {
        self.sync_streaming_assistant_component(stop_reason, error_message);
        self.render_state_mut().streaming_component = None;
        self.render_state_mut().streaming_message = None;
    }

    /// Drain any pending agent events from the channel and handle them.
    pub(super) fn drain_pending_agent_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = self.agent_events.as_mut() {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            self.handle_agent_event(event);
        }
    }

    /// Drive the event loop while streaming is active, handling both
    /// terminal events (keyboard input) and agent events (streaming text,
    /// tool calls, done signals).
    pub(super) async fn process_agent_events_until_done(&mut self) -> InteractiveResult<()> {
        while self.is_streaming {
            let terminal_events = self.events.as_mut();
            let agent_events = self.agent_events.as_mut();
            let spinner_tick = tokio::time::sleep(Duration::from_millis(80));
            tokio::pin!(spinner_tick);

            tokio::select! {
                // Terminal input events
                Some(event) = async {
                    match terminal_events {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    match event {
                        TerminalEvent::Resize(_, _) => {
                            self.ui.force_render();
                        }
                        TerminalEvent::Paste(data) | TerminalEvent::Raw(data) => {
                            self.ui.handle_raw_input(&data);
                            self.sync_bash_mode_from_editor();
                            self.refresh_ui();
                        }
                        TerminalEvent::Key(key) => {
                            // During streaming, Ctrl-C can interrupt
                            if key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                self.handle_ctrl_c();
                                if self.shutdown_requested {
                                    self.is_streaming = false;
                                    break;
                                }
                            }
                            // Queue text input for after streaming
                            if key.code == KeyCode::Enter
                                && !key.modifiers.contains(KeyModifiers::SHIFT)
                            {
                                let text = self.editor_text();
                                let text = text.trim().to_string();
                                if !text.is_empty() {
                                    self.push_editor_history(&text);
                                    self.clear_editor();
                                    self.steering_queue.push_back(text);
                                    self.sync_pending_render_state();
                                    self.refresh_ui();
                                }
                            } else if key.code == KeyCode::F(9)
                                || (key.code == KeyCode::Enter
                                    && key.modifiers.contains(KeyModifiers::ALT))
                            {
                                let text = self.editor_text();
                                let text = text.trim().to_string();
                                if !text.is_empty() {
                                    self.push_editor_history(&text);
                                    self.clear_editor();
                                    self.follow_up_queue.push_back(text);
                                    self.sync_pending_render_state();
                                    self.refresh_ui();
                                }
                            } else if key.code == KeyCode::F(10) {
                                self.handle_dequeue();
                                self.refresh_ui();
                            } else {
                                self.ui.handle_key(&key);
                                self.sync_bash_mode_from_editor();
                                self.refresh_ui();
                            }
                        }
                    }
                },
                // Agent streaming events
                Some(agent_event) = async {
                    match agent_events {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.handle_agent_event(agent_event);
                    self.refresh_ui();
                },
                _ = &mut spinner_tick => {
                    if self.status_loader.is_some() {
                        self.refresh_ui();
                    }
                },
                // Both channels closed
                else => {
                    self.is_streaming = false;
                    break;
                }
            }
        }

        // Clean up agent events channel
        self.agent_events = None;
        Ok(())
    }

    pub(super) fn take_last_submitted_text(&mut self) -> String {
        self.pending_working_message
            .take()
            .unwrap_or_else(|| String::new())
    }

    pub(super) fn sync_static_sections(&mut self) {
        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_status_container();
        self.rebuild_footer();
    }

    pub(super) fn refresh_ui(&mut self) {
        self.rebuild_chat_container();
        self.rebuild_pending_container();
        self.rebuild_status_container();
        self.rebuild_footer();
        // Always use differential render — never clear scrollback
        self.ui.render();
    }

    pub(super) fn rebuild_header(&mut self) {
        self.header_lines.clear();
        if !self.options.quiet_startup {
            let dim = "\x1b[90m";
            let reset = "\x1b[0m";
            let bold = "\x1b[1m";
            let cyan = "\x1b[36m";
            self.header_lines.push(format!(
                "{bold}{cyan}BB-Agent{reset} v{}",
                self.version
            ));
            self.header_lines.push(format!(
                "{dim}Ctrl-C exit . / commands . ! bash . F2 thinking . /help for more{reset}"
            ));
        }

        if let Ok(mut header) = self.header_container.lock() {
            header.clear();
            if !self.header_lines.is_empty() {
                header.add(Box::new(Text::new(&self.header_lines.join("\n"))));
                header.add(Box::new(Spacer::new(1)));
            }
        }
    }

    pub(super) fn rebuild_chat_container(&mut self) {
        let lines = self.chat_render_lines();
        Self::replace_container_lines(&self.chat_container, &lines);
    }

    pub(super) fn rebuild_pending_container(&mut self) {
        self.sync_pending_render_state();
        let lines = self.pending_render_lines();
        Self::replace_container_lines(&self.pending_messages_container, &lines);
    }

    pub(super) fn rebuild_status_container(&mut self) {
        if let Ok(mut container) = self.status_container.lock() {
            if let Some((style, message)) = &self.status_loader {
                let mut reused = false;
                if container.children.len() == 1 {
                    if let Some(loader) = container.children[0]
                        .as_any_mut()
                        .downcast_mut::<StatusLoaderComponent>()
                    {
                        if &loader.style == style {
                            loader.set_message(message.clone());
                            reused = true;
                        } else {
                            loader.stop();
                        }
                    }
                }
                if !reused {
                    container.clear();
                    container.add(Box::new(StatusLoaderComponent::new(*style, message.clone())));
                }
                return;
            }

            let recent = self
                .status_lines
                .iter()
                .rev()
                .take(3)
                .cloned()
                .collect::<Vec<_>>();
            let mut recent = recent;
            recent.reverse();

            if recent.is_empty() {
                if let Some(loader) = container
                    .children
                    .get_mut(0)
                    .and_then(|child| child.as_any_mut().downcast_mut::<StatusLoaderComponent>())
                {
                    loader.stop();
                }
                container.clear();
                return;
            }

            let text = recent.join("\n");
            if container.children.len() == 1 {
                if let Some(existing) = container.children[0].as_any_mut().downcast_mut::<Text>() {
                    existing.set(&text);
                    return;
                }
                if let Some(loader) = container.children[0]
                    .as_any_mut()
                    .downcast_mut::<StatusLoaderComponent>()
                {
                    loader.stop();
                }
            }
            container.clear();
            container.add(Box::new(Text::new(&text)));
        }
    }

    pub(super) fn footer_usage_totals(&self) -> (u64, u64, u64, u64, f64) {
        let mut total_input = 0_u64;
        let mut total_output = 0_u64;
        let mut total_cache_read = 0_u64;
        let mut total_cache_write = 0_u64;
        let mut total_cost = 0.0_f64;

        if let Ok(rows) = store::get_entries(&self.session_setup.conn, &self.session_setup.session_id) {
            for row in rows {
                if let Ok(entry) = store::parse_entry(&row) {
                    if let bb_core::types::SessionEntry::Message {
                        message: bb_core::types::AgentMessage::Assistant(message),
                        ..
                    } = entry
                    {
                        total_input += message.usage.input;
                        total_output += message.usage.output;
                        total_cache_read += message.usage.cache_read;
                        total_cache_write += message.usage.cache_write;
                        total_cost += message.usage.cost.total;
                    }
                }
            }
        }

        (
            total_input,
            total_output,
            total_cache_read,
            total_cache_write,
            total_cost,
        )
    }

    pub(super) fn available_provider_count(&self) -> usize {
        crate::login::authenticated_providers().len()
    }

    pub(super) fn rebuild_footer(&mut self) {
        self.footer_data_provider
            .set_cwd(self.controller.runtime_host.cwd().to_path_buf());
        self.footer_data_provider
            .set_available_provider_count(self.available_provider_count());

        let (input_tokens, output_tokens, cache_read, cache_write, cost) = self.footer_usage_totals();
        let context_usage = self.controller.runtime_host.runtime().get_context_usage();
        let context_percent = context_usage
            .as_ref()
            .and_then(|usage| usage.percent.map(|p| p as f64));
        let context_window = context_usage
            .as_ref()
            .map(|usage| usage.context_window as u64)
            .unwrap_or(self.session_setup.model.context_window);
        let session_row = store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
            .ok()
            .flatten();

        let footer = Footer::new(FooterData {
            model_name: self.session_setup.model.id.clone(),
            provider: self.session_setup.model.provider.clone(),
            cwd: self.controller.runtime_host.cwd().display().to_string(),
            git_branch: self.footer_data_provider.get_git_branch(),
            session_name: session_row.and_then(|row| row.name),
            input_tokens,
            output_tokens,
            cache_read,
            cache_write,
            cost,
            context_percent,
            context_window,
            auto_compact: true,
            thinking_level: if self.session_setup.model.reasoning {
                Some(self.session_setup.thinking_level.clone())
            } else {
                None
            },
            available_provider_count: self.footer_data_provider.get_available_provider_count(),
        });

        self.footer_lines = footer.render(self.ui.columns());
        Self::replace_container_lines(&self.footer_container, &self.footer_lines);
    }

    pub(super) fn render_widgets(&mut self) {
        // No extra spacing around editor — pi doesn't have it
        self.widgets_above_lines = vec![];
        self.widgets_below_lines = vec![];
        Self::replace_container_lines(&self.widget_container_above, &self.widgets_above_lines);
        Self::replace_container_lines(&self.widget_container_below, &self.widgets_below_lines);
    }

    pub(super) fn replace_container_lines(container: &Arc<Mutex<Container>>, lines: &[String]) {
        if let Ok(mut container) = container.lock() {
            container.clear();
            if lines.is_empty() {
                return;
            }
            container.add(Box::new(Text::new(&lines.join("\n"))));
        }
    }

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

    pub(super) fn handle_escape(&mut self) {
        // Priority 1: dismiss overlay
        if self.ui.has_overlay() {
            self.ui.hide_overlay();
            return;
        }
        // Priority 2: abort loading animation
        if self.status_loader.is_some() {
            self.status_loader = None;
            self.show_status("Aborted loading");
            return;
        }
        // Priority 3: cancel bash run
        if self.is_bash_running {
            self.is_bash_running = false;
            self.show_warning("Canceled bash placeholder run");
            return;
        }
        // Priority 4: exit bash mode
        if self
            .is_bash_mode
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
        if self.is_streaming {
            self.is_streaming = false;
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
            .last_escape_time
            .map(|last| now.saturating_duration_since(last) < Duration::from_millis(500))
            .unwrap_or(false);
        if activate {
            self.show_tree_selector();
            self.last_escape_time = None;
        } else {
            self.last_escape_time = Some(now);
        }
    }

    pub(super) fn handle_ctrl_c(&mut self) {
        // If streaming, abort and show "Aborted"
        if self.is_streaming {
            self.is_streaming = false;
            self.show_warning("Aborted");
            self.last_sigint_time = Some(Instant::now());
            return;
        }
        // If editor has text, clear it
        if !self.editor_text().trim().is_empty() {
            self.clear_editor();
            self.show_status("Editor cleared");
            self.last_sigint_time = Some(Instant::now());
            return;
        }
        // Double Ctrl-C -> shutdown
        let now = Instant::now();
        let is_double = self
            .last_sigint_time
            .map(|last| now.saturating_duration_since(last) < Duration::from_millis(500))
            .unwrap_or(false);
        self.last_sigint_time = Some(now);

        if is_double {
            self.shutdown_requested = true;
            self.show_warning("Exiting interactive mode");
        } else {
            self.show_status("Interrupt requested. Press Ctrl-C again to exit.");
        }
    }

    pub(super) fn handle_ctrl_d(&mut self) {
        if self.editor_text().trim().is_empty() {
            self.shutdown_requested = true;
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
        self.tool_output_expanded = !self.tool_output_expanded;
        let state_label = if self.tool_output_expanded {
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
        self.hide_thinking_block = !self.hide_thinking_block;
        let state_label = if self.hide_thinking_block {
            "hidden"
        } else {
            "expanded"
        };
        self.show_status(format!("thinking block {state_label}"));

        let hide_thinking_block = self.hide_thinking_block;
        let hidden_thinking_label = self.hidden_thinking_label.clone();
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
        self.follow_up_queue.push_back(text);
        self.sync_pending_render_state();
        self.show_status("Queued follow-up message");
    }

    pub(super) fn handle_dequeue(&mut self) {
        // Pop from follow-up queue first, then steering queue
        let popped = if let Some(text) = self.follow_up_queue.pop_back() {
            Some(text)
        } else {
            self.steering_queue.pop_back()
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

    pub(super) fn show_settings_selector(&mut self) {
        let _ = self.controller.commands.show_settings_selector();
        self.show_placeholder("settings selector");
    }

    pub(super) fn handle_model_command(&mut self, search_term: Option<&str>) {
        let Some(search_term) = search_term.map(str::trim).filter(|s| !s.is_empty()) else {
            self.show_model_selector(None);
            return;
        };

        if let Some(model) = self.find_exact_model_match(search_term) {
            self.apply_model_selection(model);
            return;
        }

        self.show_model_selector(Some(search_term));
    }

    pub(super) fn build_model_registry(&self) -> ModelRegistry {
        let mut registry = ModelRegistry::new();
        let settings = bb_core::settings::Settings::load_merged(&self.controller.runtime_host.cwd());
        registry.load_custom_models(&settings);
        registry
    }

    pub(super) fn get_model_candidates(&self) -> Vec<Model> {
        let current_provider = self.session_setup.model.provider.clone();
        let available = crate::login::authenticated_providers();
        let has_any_available = !available.is_empty();

        self.build_model_registry()
            .list()
            .iter()
            .filter(|model| {
                !has_any_available
                    || available.iter().any(|provider| provider == &model.provider)
                    || model.provider == current_provider
            })
            .cloned()
            .collect()
    }

    pub(super) fn find_exact_model_match(&self, search_term: &str) -> Option<Model> {
        let needle = search_term.trim().to_ascii_lowercase();
        self.get_model_candidates().into_iter().find(|model| {
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            model.id.eq_ignore_ascii_case(&needle)
                || model.name.eq_ignore_ascii_case(&needle)
                || provider_id == needle
                || provider_colon_id == needle
        })
    }

    pub(super) fn apply_model_selection(&mut self, model: Model) {
        let api_key = crate::login::resolve_api_key(&model.provider).unwrap_or_default();
        let base_url = model.base_url.clone().unwrap_or_else(|| match model.api {
            ApiType::AnthropicMessages => "https://api.anthropic.com".to_string(),
            ApiType::GoogleGenerative => "https://generativelanguage.googleapis.com".to_string(),
            _ => "https://api.openai.com/v1".to_string(),
        });
        let new_provider: Box<dyn bb_provider::Provider> = match model.api {
            ApiType::AnthropicMessages => Box::new(bb_provider::anthropic::AnthropicProvider::new()),
            ApiType::GoogleGenerative => Box::new(bb_provider::google::GoogleProvider::new()),
            _ => Box::new(bb_provider::openai::OpenAiProvider::new()),
        };
        let display = format!("{}/{}", model.provider, model.id);

        self.controller.runtime_host.session_mut().set_model(ModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            reasoning: model.reasoning,
        });
        self.controller.runtime_host.runtime_mut().model = Some(RuntimeModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            context_window: model.context_window as usize,
        });
        self.session_setup.model = model;
        self.session_setup.provider = new_provider;
        self.session_setup.api_key = api_key;
        self.session_setup.base_url = base_url;
        self.options.model_display = Some(display.clone());
        self.show_status(format!("Model: {display}"));
        self.rebuild_footer();
    }

    pub(super) fn process_overlay_actions(&mut self) {
        let action = self
            .ui
            .topmost_overlay_as_mut::<ModelSelectorOverlay>()
            .and_then(|overlay| overlay.take_action());

        match action {
            Some(ModelSelectorOverlayAction::Selected(selection)) => {
                self.ui.hide_overlay();
                if let Some(model) = self
                    .get_model_candidates()
                    .into_iter()
                    .find(|m| m.provider == selection.provider && m.id == selection.model_id)
                {
                    self.apply_model_selection(model);
                } else {
                    self.show_warning(format!(
                        "Model not found: {}/{}",
                        selection.provider, selection.model_id
                    ));
                }
            }
            Some(ModelSelectorOverlayAction::Cancelled) => {
                self.ui.hide_overlay();
                self.show_status("Canceled model selector");
            }
            None => {}
        }
    }

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
        let session_id = &self.session_setup.session_id;
        let model = self.options.model_display.as_deref().unwrap_or("unknown");
        let cwd = self.session_setup.tool_ctx.cwd.display().to_string();
        let msg_count = self.chat_lines.len() + self.render_state().chat_items.len();
        self.chat_lines.push(format!("Session ID:   {session_id}"));
        self.chat_lines.push(format!("Model:        {model}"));
        self.chat_lines.push(format!("Working dir:  {cwd}"));
        self.chat_lines.push(format!("Messages:     {msg_count}"));
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
            self.chat_lines.push(line.to_string());
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
        self.chat_lines.clear();
        self.pending_lines.clear();
        self.steering_queue.clear();
        self.follow_up_queue.clear();
        self.compaction_queued_messages.clear();
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
                self.chat_lines.clear();
                self.pending_lines.clear();
                self.compaction_queued_messages.clear();
                self.render_state_mut().chat_items.clear();
                self.render_state_mut().pending_items.clear();
                self.show_status(format!("New session created: {new_id}"));
            }
            Err(e) => {
                self.show_status(format!("Failed to create new session: {e}"));
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
            self.chat_lines.push(line.to_string());
        }
    }

    pub(super) fn check_auto_compaction(&mut self) {
        let session_id = self.session_setup.session_id.clone();
        let settings = bb_core::types::CompactionSettings::default();
        if let Ok(entries) = store::get_entries(&self.session_setup.conn, &session_id) {
            let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
            let window = self.session_setup.model.context_window;
            if compaction::should_compact(total_tokens, window, &settings) {
                self.chat_lines.push(format!(
                    "[c] Auto-compaction triggered ({total_tokens} tokens, window {window})"
                ));
                // Prepare and note - full async LLM summarization deferred to future wave
                if let Some(prep) = compaction::prepare_compaction(&entries, &settings) {
                    self.chat_lines.push(format!(
                        "[c] {} messages to summarize, {} kept",
                        prep.messages_to_summarize.len(),
                        prep.kept_messages.len()
                    ));
                }
            }
        }
    }

    pub(super) fn handle_compact_command(&mut self, instructions: Option<&str>) {
        self.is_compacting = true;
        let session_id = self.session_setup.session_id.clone();
        match store::get_entries(&self.session_setup.conn, &session_id) {
            Ok(entries) => {
                let settings = bb_core::types::CompactionSettings::default();
                let total_tokens: u64 = entries.iter().map(compaction::estimate_tokens_row).sum();
                match compaction::prepare_compaction(&entries, &settings) {
                    Some(prep) => {
                        let to_summarize = prep.messages_to_summarize.len();
                        let kept = prep.kept_messages.len();
                        self.chat_lines.push(format!(
                            "Compaction: {total_tokens} estimated tokens, {to_summarize} messages to summarize, {kept} kept"
                        ));
                        if let Some(inst) = instructions {
                            self.chat_lines.push(format!("Instructions: {inst}"));
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
        self.is_compacting = false;
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
        self.shutdown_requested = true;
        self.show_status("Shutdown requested");
    }

    pub(super) fn handle_bash_command(&mut self, command: &str, excluded_from_context: bool) {
        let label = if excluded_from_context { "bash(excluded)" } else { "bash" };
        self.chat_lines.push(format!("{label}> {command}"));
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
                        self.chat_lines.push(line.to_string());
                    }
                }
                if !stderr.is_empty() {
                    for line in stderr.lines() {
                        self.chat_lines.push(format!("stderr: {line}"));
                    }
                }
                if !out.status.success() {
                    self.chat_lines.push(format!("exit code: {}", out.status.code().unwrap_or(-1)));
                }
            }
            Err(e) => {
                self.chat_lines.push(format!("Failed to execute command: {e}"));
            }
        }
    }

    pub(super) fn flush_pending_bash_components(&mut self) {
        while let Some(line) = self.pending_bash_components.pop_front() {
            self.chat_lines.push(line);
        }
    }

    pub(super) fn is_extension_command(&self, text: &str) -> bool {
        text.starts_with("/ext") || text.starts_with("/extension")
    }

    pub(super) fn queue_compaction_message(&mut self, text: String, kind: QueuedMessageKind) {
        self.compaction_queued_messages
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
        self.ui.show_overlay(component);
        self.show_status("Opened model selector");
    }

    pub(super) fn show_placeholder(&mut self, label: &str) {
        self.show_status(format!("TODO: {label}"));
    }

    pub(super) fn merge_tool_args_delta(current: &serde_json::Value, args_delta: &str) -> serde_json::Value {
        let raw = match current {
            serde_json::Value::String(existing) => format!("{existing}{args_delta}"),
            serde_json::Value::Null => args_delta.to_string(),
            other => return other.clone(),
        };

        serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap_or_else(|_| serde_json::Value::String(raw))
    }

    pub(super) fn handle_agent_event(&mut self, event: AgentLoopEvent) {
        match event {
            AgentLoopEvent::TurnStart { .. } => {
                self.is_streaming = true;
                let loader_message = self
                    .pending_working_message
                    .take()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| self.default_working_message.to_string());
                self.status_loader = Some((StatusLoaderStyle::Accent, loader_message));
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                self.streaming_tool_calls.clear();
                self.sync_streaming_assistant_component(None, None);
                self.refresh_ui();
            }
            AgentLoopEvent::TextDelta { text } => {
                self.streaming_text.push_str(&text);
                self.update_streaming_display();
            }
            AgentLoopEvent::ThinkingDelta { text } => {
                self.streaming_thinking.push_str(&text);
                // Match pi better: thinking may arrive before text, so create/update
                // the streaming assistant component even when text is still empty.
                self.update_streaming_display();
            }
            AgentLoopEvent::ToolCallStart { id, name } => {
                if !self.streaming_tool_calls.iter().any(|call| call.id == id) {
                    self.streaming_tool_calls.push(ToolCallContent {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: serde_json::Value::Null,
                    });
                }
                self.sync_streaming_assistant_component(None, None);

                let args = serde_json::Value::Null;
                let mut component = components::tool_execution::ToolExecutionComponent::new(
                    name,
                    id.clone(),
                    args,
                    components::tool_execution::ToolExecutionOptions {
                        show_images: self.render_state().show_images,
                    },
                );
                component.set_expanded(self.tool_output_expanded);
                self.render_state_mut().chat_items.push(ChatItem::ToolExecution(component.clone()));
                self.render_state_mut().pending_tools.insert(id, component);
                self.refresh_ui();
            }
            AgentLoopEvent::ToolCallDelta { id, args_delta } => {
                if let Some(call) = self.streaming_tool_calls.iter_mut().find(|call| call.id == id) {
                    call.arguments = Self::merge_tool_args_delta(&call.arguments, &args_delta);
                }
                if let Some(component) = self.render_state_mut().pending_tools.get_mut(&id) {
                    let current = component.args().clone();
                    let new_args = Self::merge_tool_args_delta(&current, &args_delta);
                    component.update_args(new_args);
                }
                let updated = self.render_state().pending_tools.get(&id).cloned();
                if let Some(updated) = updated {
                    let id_clone = id.clone();
                    if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                        .find(|item| matches!(item, ChatItem::ToolExecution(tc) if tc.tool_call_id() == id_clone))
                    {
                        *chat_item = ChatItem::ToolExecution(updated);
                    }
                }
                self.sync_streaming_assistant_component(None, None);
                self.refresh_ui();
            }
            AgentLoopEvent::ToolExecuting { id, .. } => {
                let updated = {
                    if let Some(component) = self.render_state_mut().pending_tools.get_mut(&id) {
                        component.mark_execution_started();
                        Some(component.clone())
                    } else {
                        None
                    }
                };
                if let Some(updated) = updated {
                    let id_clone = id.clone();
                    if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                        .find(|item| matches!(item, ChatItem::ToolExecution(tc) if tc.tool_call_id() == id_clone))
                    {
                        *chat_item = ChatItem::ToolExecution(updated);
                    }
                }
                self.refresh_ui();
            }
            AgentLoopEvent::ToolResult {
                id,
                name: _,
                content,
                details,
                artifact_path,
                is_error,
            } => {
                let updated = {
                    if let Some(component) = self.render_state_mut().pending_tools.get_mut(&id) {
                        let mut blocks: Vec<components::tool_execution::ToolResultBlock> = content
                            .into_iter()
                            .map(|block| match block {
                                bb_core::types::ContentBlock::Text { text } => {
                                    components::tool_execution::ToolResultBlock {
                                        r#type: "text".to_string(),
                                        text: Some(text),
                                        data: None,
                                        mime_type: None,
                                    }
                                }
                                bb_core::types::ContentBlock::Image { data, mime_type } => {
                                    components::tool_execution::ToolResultBlock {
                                        r#type: "image".to_string(),
                                        text: None,
                                        data: Some(data),
                                        mime_type: Some(mime_type),
                                    }
                                }
                            })
                            .collect();
                        if let Some(path) = artifact_path {
                            blocks.push(components::tool_execution::ToolResultBlock {
                                r#type: "text".to_string(),
                                text: Some(format!("Full output: {path}")),
                                data: None,
                                mime_type: None,
                            });
                        }
                        let result = components::tool_execution::ToolExecutionResult {
                            content: blocks,
                            is_error,
                            details,
                        };
                        component.update_result(result, false);
                        Some(component.clone())
                    } else {
                        None
                    }
                };
                if let Some(updated) = updated {
                    let id_clone = id.clone();
                    if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                        .find(|item| matches!(item, ChatItem::ToolExecution(tc) if tc.tool_call_id() == id_clone))
                    {
                        *chat_item = ChatItem::ToolExecution(updated);
                    }
                }
                self.render_state_mut().pending_tools.remove(&id);
                self.refresh_ui();
            }
            AgentLoopEvent::TurnEnd { .. } => {
                for component in self.render_state_mut().pending_tools.values_mut() {
                    component.set_args_complete();
                }
                let updated_pending = self
                    .render_state()
                    .pending_tools
                    .iter()
                    .map(|(id, component)| (id.clone(), component.clone()))
                    .collect::<Vec<_>>();
                for (id, updated) in updated_pending {
                    if let Some(chat_item) = self.render_state_mut().chat_items.iter_mut().rev()
                        .find(|item| matches!(item, ChatItem::ToolExecution(tc) if tc.tool_call_id() == id))
                    {
                        *chat_item = ChatItem::ToolExecution(updated);
                    }
                }
                self.finalize_streaming_assistant_message(None, None);
                self.refresh_ui();
            }
            AgentLoopEvent::AssistantDone => {
                self.is_streaming = false;
                self.status_loader = None;
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                self.streaming_tool_calls.clear();
                self.pending_working_message = None;
                self.render_state_mut().streaming_component = None;
                self.render_state_mut().streaming_message = None;
                self.render_state_mut().pending_tools.clear();
                self.check_auto_compaction();
                if let Some(queued) = self.steering_queue.pop_front() {
                    self.chat_lines.push(format!("queued(steer)> {queued}"));
                    self.pending_working_message = Some(queued);
                }
                self.rebuild_footer();
                self.refresh_ui();
            }
            AgentLoopEvent::Error { message } => {
                self.is_streaming = false;
                self.status_loader = None;
                self.finalize_streaming_assistant_message(
                    Some(components::assistant_message::AssistantStopReason::Error),
                    Some(message.clone()),
                );
                for component in self.render_state_mut().pending_tools.values_mut() {
                    component.update_result(
                        components::tool_execution::ToolExecutionResult {
                            content: vec![components::tool_execution::ToolResultBlock {
                                r#type: "text".to_string(),
                                text: Some(message.clone()),
                                data: None,
                                mime_type: None,
                            }],
                            is_error: true,
                            details: None,
                        },
                        false,
                    );
                }
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                self.streaming_tool_calls.clear();
                self.render_state_mut().pending_tools.clear();
                self.rebuild_footer();
                self.refresh_ui();
            }
        }
    }

    pub(super) fn show_warning(&mut self, message: impl Into<String>) {
        let message = message.into();
        let dim = "\x1b[90m";
        let yellow = "\x1b[33m";
        let reset = "\x1b[0m";
        self.status_loader = None;
        self.render_state_mut().last_status = Some(format!("{yellow}[!]{reset} {dim}{message}{reset}"));
        self.status_lines = vec![format!("{yellow}[!]{reset} {dim}{message}{reset}")];
    }

    pub(super) fn show_status(&mut self, message: impl Into<String>) {
        let message = message.into();
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        self.status_loader = None;
        self.render_state_mut().last_status = Some(format!("{dim}{message}{reset}"));
        self.status_lines = vec![format!("{dim}{message}{reset}")];
    }
}
