use super::*;

impl InteractiveMode {
    pub fn set_on_input_callback<F>(&mut self, callback: F)
    where
        F: FnMut(String) + Send + 'static,
    {
        self.on_input_callback = Some(Box::new(callback));
    }

    pub async fn init(&mut self) -> InteractiveResult<()> {
        if self.is_initialized {
            return Ok(());
        }

        self.changelog_markdown = self.get_changelog_for_display();

        self.ui.root.add(Box::new(SharedContainer::new(
            self.header_container.clone(),
        )));
        self.ui
            .root
            .add(Box::new(SharedContainer::new(self.chat_container.clone())));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.pending_messages_container.clone(),
        )));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.status_container.clone(),
        )));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.widget_container_above.clone(),
        )));
        self.ui
            .root
            .add(Box::new(SharedEditorWrapper::new(self.editor.clone())));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.widget_container_below.clone(),
        )));
        self.ui.root.add(Box::new(SharedContainer::new(
            self.footer_container.clone(),
        )));
        self.ui.set_focus(Some(5));

        self.rebuild_header();
        self.render_widgets();
        self.rebuild_footer();
        self.sync_static_sections();

        self.setup_key_handlers();
        self.setup_editor_submit_handler();

        self.events = Some(self.ui.start());
        self.is_initialized = true;

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

        while !self.shutdown_requested {
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
            if self.shutdown_requested {
                return Ok(None);
            }

            // Use tokio::select! to handle both terminal and agent events
            tokio::select! {
                terminal_event = async {
                    match self.events.as_mut() {
                        Some(events) => events.recv().await,
                        None => None,
                    }
                } => {
                    let Some(event) = terminal_event else {
                        self.shutdown_requested = true;
                        return Ok(None);
                    };

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
            }
        }
    }


    pub(super) fn get_session_leaf(&self) -> Option<bb_core::types::EntryId> {
        store::get_session(&self.session_setup.conn, &self.session_setup.session_id)
            .ok()
            .flatten()
            .and_then(|s| s.leaf_id.map(bb_core::types::EntryId))
    }

    /// Run the full streaming turn loop: stream from provider, execute tools, loop until done.
    pub(super) async fn run_streaming_turn_loop(&mut self) -> InteractiveResult<()> {
        let (tx, rx) = mpsc::unbounded_channel::<AgentLoopEvent>();
        self.agent_events = Some(rx);

        let mut turn_index: u32 = 0;

        loop {
            let _ = tx.send(AgentLoopEvent::TurnStart { turn_index });
            self.drain_pending_agent_events();
            self.refresh_ui();

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

            let options = bb_provider::RequestOptions {
                api_key: self.session_setup.api_key.clone(),
                base_url: self.session_setup.base_url.clone(),
                headers: std::collections::HashMap::new(),
                cancel: tokio_util::sync::CancellationToken::new(),
            };

            // Spawn provider streaming in background so tokens arrive while we render
            let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
            let provider_tx = tx.clone();
            let provider = &self.session_setup.provider;
            
            // We can't move provider into spawn, so run stream inline but 
            // forward events through the agent channel for the main loop
            let stream_result = provider.stream(request, options, stream_tx).await;

            if let Err(e) = stream_result {
                let raw = format!("{e}");
                let clean = raw.lines().next().unwrap_or(&raw).to_string();
                let _ = tx.send(AgentLoopEvent::Error { message: clean });
                let _ = tx.send(AgentLoopEvent::AssistantDone);
                break;
            }

            // Process stream events, forwarding as agent events with live rendering
            let mut all_events = Vec::new();
            while let Some(event) = stream_rx.recv().await {
                match &event {
                    bb_provider::StreamEvent::TextDelta { text } => {
                        let _ = tx.send(AgentLoopEvent::TextDelta { text: text.clone() });
                        self.drain_pending_agent_events();
                        self.refresh_ui();
                    }
                    bb_provider::StreamEvent::ThinkingDelta { text } => {
                        let _ = tx.send(AgentLoopEvent::ThinkingDelta { text: text.clone() });
                        self.drain_pending_agent_events();
                        self.refresh_ui();
                    }
                    bb_provider::StreamEvent::ToolCallStart { id, name } => {
                        let _ = tx.send(AgentLoopEvent::ToolCallStart { id: id.clone(), name: name.clone() });
                        self.drain_pending_agent_events();
                        self.refresh_ui();
                    }
                    bb_provider::StreamEvent::ToolCallDelta { id, arguments_delta } => {
                        let _ = tx.send(AgentLoopEvent::ToolCallDelta { id: id.clone(), args_delta: arguments_delta.clone() });
                        self.drain_pending_agent_events();
                        self.refresh_ui();
                    }
                    bb_provider::StreamEvent::Error { message } => {
                        let _ = tx.send(AgentLoopEvent::Error { message: message.clone() });
                        self.drain_pending_agent_events();
                        self.refresh_ui();
                    }
                    _ => {}
                }
                all_events.push(event);
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
                usage: bb_core::types::Usage {
                    input: collected.input_tokens,
                    output: collected.output_tokens,
                    cache_read: collected.cache_read_tokens,
                    cache_write: collected.cache_write_tokens,
                    total_tokens: collected.input_tokens
                        + collected.output_tokens
                        + collected.cache_read_tokens
                        + collected.cache_write_tokens,
                    ..Default::default()
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

            // Execute tool calls
            let cancel = tokio_util::sync::CancellationToken::new();
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

        self.is_streaming = false;
        Ok(())
    }
}
