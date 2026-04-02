use super::*;

use bb_tools::ToolContext;
use crate::turn_runner::{self, TurnConfig, TurnEvent};

impl InteractiveMode {
    pub fn set_on_input_callback<F>(&mut self, callback: F)
    where
        F: FnMut(String) + Send + 'static,
    {
        self.on_input_callback = Some(Box::new(callback));
    }

    pub async fn init(&mut self) -> InteractiveResult<()> {
        if self.interaction.is_initialized {
            return Ok(());
        }

        self.changelog_markdown = self.get_changelog_for_display();

        self.ui.tui.root.add(Box::new(SharedContainer::new(
            self.ui.header_container.clone(),
        )));
        self.ui.tui
            .root
            .add(Box::new(SharedContainer::new(self.ui.chat_container.clone())));
        self.ui.tui.root.add(Box::new(SharedContainer::new(
            self.ui.pending_messages_container.clone(),
        )));
        self.ui.tui.root.add(Box::new(SharedContainer::new(
            self.ui.status_container.clone(),
        )));
        self.ui.tui.root.add(Box::new(SharedContainer::new(
            self.ui.widget_container_above.clone(),
        )));
        self.ui.tui
            .root
            .add(Box::new(SharedEditorWrapper::new(self.ui.editor.clone())));
        self.ui.tui.root.add(Box::new(SharedContainer::new(
            self.ui.widget_container_below.clone(),
        )));
        self.ui.tui.root.add(Box::new(SharedContainer::new(
            self.ui.footer_container.clone(),
        )));
        self.ui.tui.set_focus(Some(5));

        self.rebuild_header();
        self.render_widgets();
        self.rebuild_footer();
        self.sync_static_sections();

        self.setup_key_handlers();
        self.setup_editor_submit_handler();

        self.events = Some(self.ui.tui.start());
        self.interaction.is_initialized = true;

        // Install SIGINT handler so Ctrl-C works even during heavy rendering.
        let sigint_flag = self.interaction.sigint_flag.clone();
        tokio::spawn(async move {
            loop {
                if tokio::signal::ctrl_c().await.is_ok() {
                    sigint_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
        });

        self.bind_current_session_extensions().await?;
        self.render_initial_messages();
        self.update_terminal_title();
        self.snapshot_chat_cache();
        self.refresh_ui();

        Ok(())
    }

    pub async fn run(&mut self) -> InteractiveResult<()> {
        self.init().await?;

        self.start_background_checks();

        if !self.options.migrated_providers.is_empty() {
            self.show_warning(format!(
                "Migrated credentials to auth.json: {}",
                self.options.migrated_providers.join(", ")
            ));
        }

        if let Some(message) = self.options.model_fallback_message.clone() {
            self.show_warning(message);
        }

        if let Some(initial_message) = self.options.initial_message.clone() {
            self.dispatch_prompt(initial_message).await?;
            self.drain_queued_messages().await?;
        }

        for message in self.options.initial_messages.clone() {
            self.dispatch_prompt(message).await?;
            self.drain_queued_messages().await?;
        }

        while !self.interaction.shutdown_requested {
            // Check SIGINT flag from signal handler.
            if self.interaction.sigint_flag.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.interaction.shutdown_requested = true;
                break;
            }
            let Some(user_input) = self.get_user_input().await? else {
                break;
            };
            self.dispatch_prompt(user_input).await?;
            self.drain_queued_messages().await?;
        }

        self.stop_ui();
        Ok(())
    }

    /// Set the agent event receiver for streaming agent loop events.
    pub fn set_agent_events(&mut self, rx: UnboundedReceiver<AgentLoopEvent>) {
        self.agent_events = Some(rx);
    }

    pub(super) async fn get_user_input(&mut self) -> InteractiveResult<Option<String>> {
        loop {
            if self.interaction.shutdown_requested {
                return Ok(None);
            }
            // Check SIGINT flag from signal handler.
            if self.interaction.sigint_flag.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.interaction.shutdown_requested = true;
                return Ok(None);
            }

            // Check for completed OAuth flows.
            self.poll_oauth_result();

            // If an OAuth flow just completed, run a verification request.
            if let Some(provider) = self.streaming.pending_oauth_verify_provider.take() {
                self.run_oauth_verification(provider).await;
            }

            // Use tokio::select! to handle terminal events, agent events,
            // and a periodic tick for background polling (OAuth results etc).
            tokio::select! {
                terminal_event = async {
                    match self.events.as_mut() {
                        Some(events) => events.recv().await,
                        None => None,
                    }
                } => {
                    let Some(event) = terminal_event else {
                        self.interaction.shutdown_requested = true;
                        return Ok(None);
                    };

                    match event {
                        TerminalEvent::Resize(_, _) => {
                            self.ui.tui.force_render();
                        }
                        TerminalEvent::Paste(data) | TerminalEvent::Raw(data) => {
                            self.ui.tui.handle_raw_input(&data);
                            self.sync_bash_mode_from_editor();
                            self.render_editor_frame();
                        }
                        TerminalEvent::Key(key) => {
                            if let Some(submitted) = self.handle_key_event(key).await? {
                                return Ok(Some(submitted));
                            }
                        }
                    }
                }
                agent_event = async {
                    match self.agent_events.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending::<Option<AgentLoopEvent>>().await,
                    }
                } => {
                    if let Some(event) = agent_event {
                        self.handle_agent_event(event);
                    }
                }
                // Periodic tick so background work (OAuth polling, etc) runs
                // even when no terminal/agent events arrive.
                // 80ms tick for spinner animation + background polling.
                _ = tokio::time::sleep(std::time::Duration::from_millis(80)) => {
                    // Spinner: rebuild status if loader is active.
                    if self.streaming.status_loader.is_some() {
                        self.rebuild_status_container();
                        self.ui.tui.render();
                    }
                    // poll_oauth_result runs at loop top on next iteration.
                }
            }
        }
    }


    /// Send a tiny test request to verify that newly-saved OAuth credentials work.
    /// Uses the model registry + parse_model_arg to pick the right default model.
    async fn run_oauth_verification(&mut self, provider: String) {
        // Resolve the fresh key we just saved.
        let api_key = match crate::login::resolve_api_key(&provider) {
            Some(k) if !k.trim().is_empty() => k,
            _ => {
                self.show_warning(format!("{provider}: saved but could not read key back"));
                return;
            }
        };

        // Use parse_model_arg to get the default model for this provider
        // (same logic used at startup: anthropic->claude-opus-4-6, openai->gpt-5.4, etc.)
        let lookup_provider = if provider == "openai-codex" { "openai" } else { &provider };
        let (_, default_model_id, _) = bb_core::agent_session::parse_model_arg(
            Some(lookup_provider),
            None,
        );

        // Find the model in registry to get api type + base_url.
        let registry = bb_provider::registry::ModelRegistry::new();
        let model = registry.find(lookup_provider, &default_model_id);

        let (test_provider, base_url, model_id) = match model {
            Some(m) => {
                let p: std::sync::Arc<dyn bb_provider::Provider> = match m.api {
                    bb_provider::registry::ApiType::AnthropicMessages => {
                        std::sync::Arc::new(bb_provider::anthropic::AnthropicProvider::new())
                    }
                    bb_provider::registry::ApiType::GoogleGenerative => {
                        std::sync::Arc::new(bb_provider::google::GoogleProvider::new())
                    }
                    _ => std::sync::Arc::new(bb_provider::openai::OpenAiProvider::new()),
                };
                let url = m.base_url.clone().unwrap_or_else(|| match m.api {
                    bb_provider::registry::ApiType::AnthropicMessages => {
                        "https://api.anthropic.com".into()
                    }
                    bb_provider::registry::ApiType::GoogleGenerative => {
                        "https://generativelanguage.googleapis.com".into()
                    }
                    _ => "https://api.openai.com/v1".into(),
                });
                (p, url, m.id.clone())
            }
            None => {
                self.show_status(format!("Logged in to {provider}."));
                return;
            }
        };

        self.show_status(format!("Verifying {provider} with {model_id}…"));
        self.refresh_ui();

        let request = bb_provider::CompletionRequest {
            system_prompt: String::new(),
            messages: vec![serde_json::json!({"role": "user", "content": "Reply with exactly: ok"})],
            tools: vec![],
            model: model_id.clone(),
            max_tokens: Some(16),
            stream: false,
            thinking: None,
        };
        let options = bb_provider::RequestOptions {
            api_key,
            base_url,
            headers: std::collections::HashMap::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
            retry_callback: None,
            max_retries: 1,
            retry_base_delay_ms: 1_000,
        };

        match test_provider.complete(request, options).await {
            Ok(_events) => {
                self.show_status(format!(
                    "Logged in to {provider} -- verified with {model_id}."
                ));
                // If this matches the current session provider, update the live key.
                let model_provider = self.session_setup.model.provider.clone();
                let matches = model_provider == provider
                    || (provider == "openai-codex" && model_provider == "openai")
                    || (provider == "openai" && model_provider == "openai-codex");
                if matches {
                    if let Some(k) = crate::login::resolve_api_key(&provider) {
                        self.session_setup.api_key = k;
                    }
                }
            }
            Err(e) => {
                let msg = format!("{e}");
                let short = msg.lines().next().unwrap_or(&msg);
                self.show_warning(format!(
                    "{provider}: credentials saved but verification failed -- {short}"
                ));
            }
        }
        self.rebuild_footer();
        self.refresh_ui();
    }


    pub(super) fn get_session_leaf(&self) -> Option<bb_core::types::EntryId> {
        turn_runner::get_leaf_raw(&self.session_setup.conn, &self.session_setup.session_id)
    }

    /// Convert a TurnEvent into an AgentLoopEvent for the existing UI event handler.
    fn turn_event_to_agent_event(event: TurnEvent) -> Option<AgentLoopEvent> {
        match event {
            TurnEvent::TurnStart { turn_index } => {
                Some(AgentLoopEvent::TurnStart { turn_index })
            }
            TurnEvent::TextDelta(text) => {
                Some(AgentLoopEvent::TextDelta { text })
            }
            TurnEvent::ThinkingDelta(text) => {
                Some(AgentLoopEvent::ThinkingDelta { text })
            }
            TurnEvent::ToolCallStart { id, name } => {
                Some(AgentLoopEvent::ToolCallStart { id, name })
            }
            TurnEvent::ToolCallDelta { id, args } => {
                Some(AgentLoopEvent::ToolCallDelta { id, args_delta: args })
            }
            TurnEvent::ToolExecuting { id, name } => {
                Some(AgentLoopEvent::ToolExecuting { id, name })
            }
            TurnEvent::ToolResult { id, name, content, details, artifact_path, is_error } => {
                Some(AgentLoopEvent::ToolResult { id, name, content, details, artifact_path, is_error })
            }
            TurnEvent::TurnEnd { turn_index } => {
                Some(AgentLoopEvent::TurnEnd { turn_index })
            }
            TurnEvent::Done { .. } => {
                Some(AgentLoopEvent::AssistantDone)
            }
            TurnEvent::Error(message) => {
                Some(AgentLoopEvent::Error { message })
            }
            TurnEvent::ContextOverflow { .. } => {
                // Handled specially by the caller, not forwarded as an agent event.
                None
            }
            TurnEvent::AutoRetryStart { attempt, max_attempts, delay_ms, error_message } => {
                Some(AgentLoopEvent::AutoRetryStart { attempt, max_attempts, delay_ms, error_message })
            }
            TurnEvent::AutoRetryEnd { success, attempt, final_error } => {
                Some(AgentLoopEvent::AutoRetryEnd { success, attempt, final_error })
            }
        }
    }

    /// Build a TurnConfig by temporarily taking ownership of session resources.
    /// Opens a sibling DB connection for the spawned turn-runner task
    /// (rusqlite::Connection is Send but not Clone).
    fn build_turn_config(&mut self) -> Result<TurnConfig, Box<dyn Error + Send + Sync>> {
        // Take tools out of session_setup (we'll put them back when the task finishes)
        let tools = std::mem::take(&mut self.session_setup.tools);

        // Reuse cached sibling connection (avoid opening new SQLite conn each turn).
        let sibling_conn = if let Some(conn) = self.session_setup.sibling_conn.clone() {
            conn
        } else {
            let conn = turn_runner::open_sibling_conn(&self.session_setup.conn)
                .map_err(|e| -> Box<dyn Error + Send + Sync> {
                    Box::<dyn Error + Send + Sync>::from(e.to_string())
                })?;
            self.session_setup.sibling_conn = Some(conn.clone());
            conn
        };

        Ok(TurnConfig {
            conn: sibling_conn,
            session_id: self.session_setup.session_id.clone(),
            system_prompt: self.session_setup.system_prompt.clone(),
            model: self.session_setup.model.clone(),
            provider: self.session_setup.provider.clone(),
            api_key: self.session_setup.api_key.clone(),
            base_url: self.session_setup.base_url.clone(),
            tools,
            tool_defs: self.session_setup.tool_defs.clone(),
            tool_ctx: ToolContext {
                cwd: self.session_setup.tool_ctx.cwd.clone(),
                artifacts_dir: self.session_setup.tool_ctx.artifacts_dir.clone(),
                on_output: None,
            },
            thinking: if self.session_setup.thinking_level == "off" {
                None
            } else {
                Some(self.session_setup.thinking_level.clone())
            },
            retry_enabled: self.session_setup.retry_enabled,
            retry_max_retries: self.session_setup.retry_max_retries,
            retry_base_delay_ms: self.session_setup.retry_base_delay_ms,
            cancel: self.abort_token.clone(),
        })
    }

    /// Run the full streaming turn loop: stream from provider, execute tools, loop until done.
    /// Processes terminal events (Esc/Ctrl-C) during streaming so user can abort.
    pub(super) async fn run_streaming_turn_loop(&mut self) -> InteractiveResult<()> {
        let (agent_tx, rx) = mpsc::unbounded_channel::<AgentLoopEvent>();
        self.agent_events = Some(rx);

        // Fresh cancellation token for this turn sequence
        self.abort_token = tokio_util::sync::CancellationToken::new();

        let turn_config = self.build_turn_config()?;

        // Spawn the turn runner in a background task.
        // run_turn takes ownership of TurnConfig and returns it when done,
        // so we can recover the tools.
        let (turn_event_tx, mut turn_event_rx) = mpsc::unbounded_channel::<TurnEvent>();
        let turn_handle = tokio::spawn(async move {
            turn_runner::run_turn(turn_config, turn_event_tx).await
        });

        // Process turn events and terminal events concurrently
        let mut aborted = false;
        let mut context_overflow = false;

        let mut spinner_interval = tokio::time::interval(std::time::Duration::from_millis(80));
        spinner_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                turn_event = turn_event_rx.recv() => {
                    let Some(event) = turn_event else {
                        // Channel closed, turn runner finished
                        break;
                    };

                    // Check for context overflow
                    if matches!(&event, TurnEvent::ContextOverflow { .. }) {
                        context_overflow = true;
                        break;
                    }

                    // Convert to AgentLoopEvent and forward
                    if let Some(agent_event) = Self::turn_event_to_agent_event(event) {
                        let _ = agent_tx.send(agent_event);
                    }
                    self.drain_pending_agent_events();
                    self.refresh_ui();
                }
                terminal_event = async {
                    match self.events.as_mut() {
                        Some(events) => events.recv().await,
                        None => std::future::pending::<Option<TerminalEvent>>().await,
                    }
                } => {
                    if let Some(event) = terminal_event {
                        match event {
                            TerminalEvent::Key(key) => {
                                // Only abort on explicit Esc press or Ctrl-C press.
                                let is_esc = key.code == KeyCode::Esc
                                    && key.modifiers == KeyModifiers::NONE;
                                let is_ctrl_c = key.code == KeyCode::Char('c')
                                    && key.modifiers == KeyModifiers::CONTROL;
                                if is_esc || is_ctrl_c {
                                    self.abort_token.cancel();
                                    aborted = true;
                                    if !self.streaming.retry_in_progress {
                                        self.show_warning("Aborted");
                                    }
                                } else {
                                    // Forward to TUI so the editor receives input during streaming.
                                    self.ui.tui.handle_key(&key);

                                    // If Enter was pressed, queue the editor text as a steer message.
                                    if key.code == KeyCode::Enter
                                        && !key.modifiers.contains(KeyModifiers::SHIFT)
                                    {
                                        let submitted = self.ui.editor.lock()
                                            .ok()
                                            .and_then(|mut e| e.try_submit());
                                        if let Some(text) = submitted {
                                            self.push_editor_history(&text);
                                            self.queues.steering_queue.push_back(text);
                                            self.sync_pending_render_state();
                                        }
                                    }
                                    self.sync_bash_mode_from_editor();
                                    self.refresh_ui();
                                }
                            }
                            TerminalEvent::Resize(_, _) => {
                                self.ui.tui.force_render();
                            }
                            TerminalEvent::Paste(data) | TerminalEvent::Raw(data) => {
                                self.ui.tui.handle_raw_input(&data);
                                self.sync_bash_mode_from_editor();
                                self.refresh_ui();
                            }
                        }
                    }
                }
                // 80ms tick for spinner animation + SIGINT check.
                _ = spinner_interval.tick() => {
                    // Check SIGINT flag (Ctrl-C from signal handler).
                    if self.interaction.sigint_flag.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        self.abort_token.cancel();
                        aborted = true;
                        if !self.streaming.retry_in_progress {
                            self.show_warning("Aborted");
                        }
                    }
                    if self.streaming.status_loader.is_some() {
                        self.rebuild_status_container();
                        self.ui.tui.render();
                    }
                }
            }

            if aborted {
                break;
            }
        }

        // Wait for turn runner to finish (with timeout to prevent hang).
        let (returned_config, turn_result) = match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            turn_handle,
        ).await {
            Ok(Ok((config, result))) => (Some(config), result),
            Ok(Err(e)) => {
                self.show_warning(format!("Turn runner task panicked: {e}"));
                (None, Ok(()))
            }
            Err(_) => {
                // Timeout waiting for turn runner — it's stuck, just proceed.
                (None, Ok(()))
            }
        };

        // Restore tools from the returned config
        if let Some(cfg) = returned_config {
            self.session_setup.tools = cfg.tools;
        }

        if aborted {
            let _ = agent_tx.send(AgentLoopEvent::AssistantDone);
            self.drain_pending_agent_events();
            self.refresh_ui();
        } else if context_overflow {
            // Handle context overflow: auto-compact and retry
            if self.handle_context_overflow().await {
                self.rebuild_chat_container();
                self.refresh_ui();
                // Retry the entire turn loop (boxed to avoid infinite-size future)
                return Box::pin(self.run_streaming_turn_loop()).await;
            } else {
                let _ = agent_tx.send(AgentLoopEvent::AssistantDone);
                self.drain_pending_agent_events();
            }
        } else {
            // Normal completion - drain any remaining events
            self.drain_pending_agent_events();
            self.refresh_ui();
        }

        let _ = turn_result;
        self.streaming.is_streaming = false;
        Ok(())
    }

    /// Run auto-compaction: summarize older messages to reclaim context space.
    /// Returns true if compaction was performed successfully.
    pub(super) async fn run_auto_compaction(&mut self) -> bool {
        // Don't compact if already compacted recently (last entry is compaction)
        let entries = match bb_session::tree::active_path(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        ) {
            Ok(e) => e,
            Err(_) => return false,
        };
        if entries.is_empty() {
            return false;
        }
        // Check if the last entry is already a compaction
        if entries.last().map(|e| e.entry_type.as_str()) == Some("compaction") {
            return false;
        }

        let settings = bb_core::types::CompactionSettings::default();
        let preparation = match compaction::prepare_compaction(&entries, &settings) {
            Some(p) => p,
            None => return false,
        };

        let tokens_before = preparation.tokens_before;
        let to_summarize_count = preparation.messages_to_summarize.len();

        self.show_status(format!(
            "Auto-compacting context… ({tokens_before} tokens, {to_summarize_count} messages to summarize)"
        ));
        self.interaction.is_compacting = true;
        self.refresh_ui();

        // Use a fresh cancel token so Esc can abort compaction
        let compact_cancel = tokio_util::sync::CancellationToken::new();
        let cancel_for_select = compact_cancel.clone();

        // Spawn the compaction LLM call
        let provider = self.session_setup.provider.clone();
        let model_id = self.session_setup.model.id.clone();
        let api_key = self.session_setup.api_key.clone();
        let base_url = self.session_setup.base_url.clone();
        let cancel_token = compact_cancel.clone();

        let mut compact_handle = tokio::spawn(async move {
            compaction::compact(
                &preparation,
                provider.as_ref(),
                &model_id,
                &api_key,
                &base_url,
                None, // no custom instructions for auto-compact
                cancel_token,
            )
            .await
        });

        // Wait for compaction while allowing Esc to cancel
        let result = loop {
            tokio::select! {
                res = &mut compact_handle => {
                    break match res {
                        Ok(Ok(r)) => Some(r),
                        Ok(Err(e)) => {
                            self.show_warning(format!("Auto-compaction failed: {e}"));
                            None
                        }
                        Err(e) => {
                            self.show_warning(format!("Auto-compaction task failed: {e}"));
                            None
                        }
                    };
                }
                terminal_event = async {
                    match self.events.as_mut() {
                        Some(events) => events.recv().await,
                        None => std::future::pending::<Option<TerminalEvent>>().await,
                    }
                } => {
                    if let Some(event) = terminal_event {
                        match event {
                            TerminalEvent::Key(key) => {
                                let is_esc = key.code == KeyCode::Esc
                                    && key.modifiers == KeyModifiers::NONE;
                                let is_ctrl_c = key.code == KeyCode::Char('c')
                                    && key.modifiers == KeyModifiers::CONTROL;
                                if is_esc || is_ctrl_c {
                                    cancel_for_select.cancel();
                                    self.show_warning("Auto-compaction cancelled");
                                    self.interaction.is_compacting = false;
                                    self.refresh_ui();
                                    return false;
                                }
                            }
                            TerminalEvent::Resize(_, _) => {
                                self.ui.tui.force_render();
                            }
                            _ => {}
                        }
                    }
                }
            }
        };

        self.interaction.is_compacting = false;

        let Some(compaction_result) = result else {
            return false;
        };

        // Save compaction entry to session
        let tokens_after_estimate = compaction::estimate_tokens_text(&compaction_result.summary);
        let compaction_entry = bb_core::types::SessionEntry::Compaction {
            base: bb_core::types::EntryBase {
                id: bb_core::types::EntryId::generate(),
                parent_id: self.get_session_leaf(),
                timestamp: chrono::Utc::now(),
            },
            summary: compaction_result.summary.clone(),
            first_kept_entry_id: bb_core::types::EntryId(compaction_result.first_kept_entry_id.clone()),
            tokens_before: compaction_result.tokens_before,
            details: None,
            from_plugin: false,
        };

        if let Err(e) = store::append_entry(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &compaction_entry,
        ) {
            self.show_warning(format!("Failed to save compaction: {e}"));
            return false;
        }

        // Show compaction summary in chat
        self.render_state_mut().add_message_to_chat(
            super::super::events::InteractiveMessage::CompactionSummary {
                summary: format!(
                    "Context compacted: {}k → {}k tokens",
                    tokens_before / 1000,
                    tokens_after_estimate / 1000,
                ),
            },
        );
        self.show_status(format!(
            "Context compacted: {}k → ~{}k tokens",
            tokens_before / 1000,
            tokens_after_estimate / 1000,
        ));
        self.refresh_ui();

        true
    }

    /// Attempt to handle a context overflow error by compacting and signalling retry.
    /// Returns true if compaction succeeded and the caller should retry.
    pub(super) async fn handle_context_overflow(&mut self) -> bool {
        self.show_warning("Context overflow detected — auto-compacting…");
        self.refresh_ui();
        self.run_auto_compaction().await
    }
}
