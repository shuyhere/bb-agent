use super::*;

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

        self.bind_current_session_extensions().await?;
        self.render_initial_messages();
        self.update_terminal_title();
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
                            self.refresh_ui();
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
                _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {
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
        store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
            .ok()
            .flatten()
            .and_then(|s| s.leaf_id.map(bb_core::types::EntryId))
    }

    /// Run the full streaming turn loop: stream from provider, execute tools, loop until done.
    /// Processes terminal events (Esc/Ctrl-C) during streaming so user can abort.
    pub(super) async fn run_streaming_turn_loop(&mut self) -> InteractiveResult<()> {
        let (tx, rx) = mpsc::unbounded_channel::<AgentLoopEvent>();
        self.agent_events = Some(rx);

        // Fresh cancellation token for this turn sequence
        self.abort_token = tokio_util::sync::CancellationToken::new();

        let mut turn_index: u32 = 0;
        let mut retry_count: u32 = 0;
        const MAX_RETRIES: u32 = 5;
        const BASE_DELAY_MS: u64 = 2000;

        loop {
            let _ = tx.send(AgentLoopEvent::TurnStart { turn_index });
            self.drain_pending_agent_events();
            self.refresh_ui();

            if self.abort_token.is_cancelled() {
                let _ = tx.send(AgentLoopEvent::AssistantDone);
                self.drain_pending_agent_events();
                break;
            }

            // Build context from session
            let ctx = bb_session::context::build_context(
                &self.session_setup.conn,
                &self.session_setup.session_id,
            ).map_err(|e| -> Box<dyn Error + Send + Sync> { Box::<dyn Error + Send + Sync>::from(e.to_string()) })?;

            let provider_messages = bb_core::agent_session::messages_to_provider(&ctx.messages);

            let request = bb_provider::CompletionRequest {
                system_prompt: self.session_setup.system_prompt.clone(),
                messages: provider_messages,
                tools: self.session_setup.tool_defs.clone(),
                model: self.session_setup.model.id.clone(),
                max_tokens: Some(self.session_setup.model.max_tokens as u32),
                stream: true,
                thinking: if self.session_setup.thinking_level == "off" { None } else { Some(self.session_setup.thinking_level.clone()) },
            };

            let cancel_token = self.abort_token.clone();
            let options = bb_provider::RequestOptions {
                api_key: self.session_setup.api_key.clone(),
                base_url: self.session_setup.base_url.clone(),
                headers: std::collections::HashMap::new(),
                cancel: cancel_token.clone(),
            };

            // Spawn provider streaming in a background task so we can select on terminal events
            let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
            let provider = self.session_setup.provider.clone();
            let stream_cancel = cancel_token.clone();
            let stream_handle = tokio::spawn(async move {
                let result = provider.stream(request, options, stream_tx).await;
                if let Err(e) = result {
                    if !stream_cancel.is_cancelled() {
                        return Err(e);
                    }
                }
                Ok(())
            });

            // Process stream events while also handling terminal input (Esc to abort)
            let mut all_events = Vec::new();
            let mut stream_done = false;
            let mut aborted = false;

            while !stream_done && !aborted {
                tokio::select! {
                    stream_event = stream_rx.recv() => {
                        let Some(event) = stream_event else {
                            stream_done = true;
                            break;
                        };
                        match &event {
                            bb_provider::StreamEvent::TextDelta { text } => {
                                let _ = tx.send(AgentLoopEvent::TextDelta { text: text.clone() });
                            }
                            bb_provider::StreamEvent::ThinkingDelta { text } => {
                                let _ = tx.send(AgentLoopEvent::ThinkingDelta { text: text.clone() });
                            }
                            bb_provider::StreamEvent::ToolCallStart { id, name } => {
                                let _ = tx.send(AgentLoopEvent::ToolCallStart { id: id.clone(), name: name.clone() });
                            }
                            bb_provider::StreamEvent::ToolCallDelta { id, arguments_delta } => {
                                let _ = tx.send(AgentLoopEvent::ToolCallDelta { id: id.clone(), args_delta: arguments_delta.clone() });
                            }
                            bb_provider::StreamEvent::Error { message } => {
                                let _ = tx.send(AgentLoopEvent::Error { message: message.clone() });
                            }
                            _ => {}
                        }
                        all_events.push(event);
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
                                    // Ignore other keys (including Enter release, arrow keys, etc).
                                    let is_esc = key.code == KeyCode::Esc
                                        && key.modifiers == KeyModifiers::NONE;
                                    let is_ctrl_c = key.code == KeyCode::Char('c')
                                        && key.modifiers == KeyModifiers::CONTROL;
                                    if is_esc || is_ctrl_c {
                                        self.abort_token.cancel();
                                        aborted = true;
                                        self.show_warning("Aborted");
                                    }
                                    // All other keys are silently consumed during streaming
                                }
                                TerminalEvent::Resize(_, _) => {
                                    self.ui.tui.force_render();
                                }
                                _ => {
                                    // Silently consume paste/raw events during streaming
                                }
                            }
                        }
                    }
                }
            }

            // Wait for stream task to finish (it should stop quickly after cancel)
            let stream_result = stream_handle.await;

            if aborted {
                let _ = tx.send(AgentLoopEvent::AssistantDone);
                self.drain_pending_agent_events();
                self.refresh_ui();
                break;
            }

            // Check for retryable provider errors
            let provider_error = match &stream_result {
                Ok(Err(e)) => Some(e.to_string()),
                Err(e) => Some(e.to_string()),
                Ok(Ok(())) => {
                    // Also check for error events in the stream
                    all_events.iter().find_map(|ev| {
                        if let bb_provider::StreamEvent::Error { message } = ev {
                            Some(message.clone())
                        } else {
                            None
                        }
                    })
                }
            };

            if let Some(ref error_msg) = provider_error {
                if is_retryable_error(error_msg) && retry_count < MAX_RETRIES {
                    let delay_ms = BASE_DELAY_MS * 2u64.pow(retry_count);
                    let delay_secs = delay_ms / 1000;
                    retry_count += 1;
                    self.show_warning(format!(
                        "Rate limited, retrying in {}s ({}/{})",
                        delay_secs, retry_count, MAX_RETRIES
                    ));
                    self.refresh_ui();

                    // Sleep with abort support (Esc cancels)
                    let retry_aborted = self.abortable_sleep(
                        std::time::Duration::from_millis(delay_ms),
                    ).await;

                    if retry_aborted {
                        self.show_warning(format!("Retry cancelled: {}", error_msg));
                        let _ = tx.send(AgentLoopEvent::AssistantDone);
                        self.drain_pending_agent_events();
                        self.refresh_ui();
                        break;
                    }

                    // Don't append failed assistant message; loop back to retry
                    continue;
                }
                // Max retries exceeded or non-retryable: fall through to normal handling
                if retry_count >= MAX_RETRIES {
                    self.show_warning(format!(
                        "Max retries ({}) exceeded: {}",
                        MAX_RETRIES, error_msg
                    ));
                }
            } else if retry_count > 0 {
                // Success after retries
                self.show_status(format!("Retry succeeded (attempt {})", retry_count + 1));
                retry_count = 0;
            }

            // Final render after stream ends
            self.refresh_ui();

            let collected = bb_provider::streaming::CollectedResponse::from_events(&all_events);

            // Build assistant message and append to session
            let mut assistant_content = Vec::new();
            if !collected.thinking.is_empty() {
                assistant_content.push(bb_core::types::AssistantContent::Thinking { thinking: collected.thinking.clone() });
            }
            if !collected.text.is_empty() {
                assistant_content.push(bb_core::types::AssistantContent::Text { text: collected.text.clone() });
            }
            for tc in &collected.tool_calls {
                let args: serde_json::Value = serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
                assistant_content.push(bb_core::types::AssistantContent::ToolCall {
                    id: tc.id.clone(), name: tc.name.clone(), arguments: args,
                });
            }
            let assistant_msg = bb_core::types::AgentMessage::Assistant(bb_core::types::AssistantMessage {
                content: assistant_content,
                provider: self.session_setup.model.provider.clone(),
                model: self.session_setup.model.id.clone(),
                usage: {
                    let inp = collected.input_tokens;
                    let out = collected.output_tokens;
                    let cr = collected.cache_read_tokens;
                    let cw = collected.cache_write_tokens;
                    let model_cost = &self.session_setup.model.cost;
                    let cost = bb_core::types::Cost {
                        input: (model_cost.input / 1_000_000.0) * inp as f64,
                        output: (model_cost.output / 1_000_000.0) * out as f64,
                        cache_read: (model_cost.cache_read / 1_000_000.0) * cr as f64,
                        cache_write: (model_cost.cache_write / 1_000_000.0) * cw as f64,
                        total: (model_cost.input / 1_000_000.0) * inp as f64
                            + (model_cost.output / 1_000_000.0) * out as f64
                            + (model_cost.cache_read / 1_000_000.0) * cr as f64
                            + (model_cost.cache_write / 1_000_000.0) * cw as f64,
                    };
                    bb_core::types::Usage {
                        input: inp,
                        output: out,
                        cache_read: cr,
                        cache_write: cw,
                        total_tokens: inp + out + cr + cw,
                        cost,
                    }
                },
                stop_reason: if collected.tool_calls.is_empty() { bb_core::types::StopReason::Stop } else { bb_core::types::StopReason::ToolUse },
                error_message: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            let asst_entry = bb_core::types::SessionEntry::Message {
                base: bb_core::types::EntryBase {
                    id: bb_core::types::EntryId::generate(),
                    parent_id: self.get_session_leaf(),
                    timestamp: chrono::Utc::now(),
                },
                message: assistant_msg,
            };
            store::append_entry(&self.session_setup.conn, &self.session_setup.session_id, &asst_entry)
                .map_err(|e| -> Box<dyn Error + Send + Sync> { Box::<dyn Error + Send + Sync>::from(e.to_string()) })?;

            let _ = tx.send(AgentLoopEvent::TurnEnd { turn_index });
            self.drain_pending_agent_events();
            self.refresh_ui();

            // If no tool calls, we're done
            if collected.tool_calls.is_empty() {
                let _ = tx.send(AgentLoopEvent::AssistantDone);
                self.drain_pending_agent_events();
                break;
            }

            // Execute tool calls (using the shared abort token so Esc cancels tools too)
            if self.abort_token.is_cancelled() {
                let _ = tx.send(AgentLoopEvent::AssistantDone);
                self.drain_pending_agent_events();
                break;
            }
            let cancel = self.abort_token.clone();
            for tc in &collected.tool_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
                let _ = tx.send(AgentLoopEvent::ToolExecuting {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                });
                self.drain_pending_agent_events();
                self.refresh_ui();

                let tool = self.session_setup.tools.iter().find(|t| t.name() == tc.name);
                let result = match tool {
                    Some(t) => t.execute(args, &self.session_setup.tool_ctx, cancel.clone()).await,
                    None => Err(bb_core::error::BbError::Tool(format!(
                        "Unknown tool: {}",
                        tc.name
                    ))),
                };
                let (content, details, artifact_path, is_error) = match result {
                    Ok(r) => (
                        r.content,
                        r.details,
                        r.artifact_path.map(|p| p.display().to_string()),
                        r.is_error,
                    ),
                    Err(e) => (
                        vec![bb_core::types::ContentBlock::Text {
                            text: format!("Error: {e}"),
                        }],
                        None,
                        None,
                        true,
                    ),
                };

                let _ = tx.send(AgentLoopEvent::ToolResult {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    content: content.clone(),
                    details: details.clone(),
                    artifact_path: artifact_path.clone(),
                    is_error,
                });
                self.drain_pending_agent_events();
                self.refresh_ui();

                let tool_result_entry = bb_core::types::SessionEntry::Message {
                    base: bb_core::types::EntryBase {
                        id: bb_core::types::EntryId::generate(),
                        parent_id: self.get_session_leaf(),
                        timestamp: chrono::Utc::now(),
                    },
                    message: bb_core::types::AgentMessage::ToolResult(
                        bb_core::types::ToolResultMessage {
                            tool_call_id: tc.id.clone(),
                            tool_name: tc.name.clone(),
                            content,
                            details,
                            is_error,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        },
                    ),
                };
                store::append_entry(
                    &self.session_setup.conn,
                    &self.session_setup.session_id,
                    &tool_result_entry,
                )
                .map_err(|e| -> Box<dyn Error + Send + Sync> {
                    Box::<dyn Error + Send + Sync>::from(e.to_string())
                })?;
            }

            turn_index += 1;
        }

        self.streaming.is_streaming = false;
        Ok(())
    }

    /// Sleep for the given duration, but abort early if the user presses Esc or Ctrl-C.
    /// Returns `true` if aborted by user, `false` if sleep completed normally.
    async fn abortable_sleep(&mut self, duration: std::time::Duration) -> bool {
        let deadline = tokio::time::Instant::now() + duration;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    return false;
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
                                    return true;
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
        }
    }
}

/// Check if an error message indicates a retryable provider error.
fn is_retryable_error(message: &str) -> bool {
    let lower = message.to_lowercase();

    // Non-retryable patterns (check first)
    if lower.contains("401 unauthorized")
        || lower.contains("400 bad request")
        || lower.contains("context length")
        || lower.contains("context window")
        || lower.contains("maximum context")
        || lower.contains("token limit")
    {
        return false;
    }

    // Retryable patterns
    let retryable_patterns = [
        "overloaded",
        "rate limit",
        "too many requests",
        "429",
        "500",
        "502",
        "503",
        "504",
        "service unavailable",
        "server error",
        "internal error",
        "network error",
        "connection error",
        "connection refused",
        "fetch failed",
        "timed out",
        "timeout",
    ];

    retryable_patterns.iter().any(|p| lower.contains(p))
}
