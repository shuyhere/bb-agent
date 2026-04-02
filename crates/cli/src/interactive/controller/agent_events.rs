use super::*;

impl InteractiveMode {
    pub(super) fn make_streaming_assistant_message(
        &self,
        stop_reason: Option<components::assistant_message::AssistantStopReason>,
        error_message: Option<String>,
    ) -> components::assistant_message::AssistantMessage {
        let mut message = assistant_message_from_parts(
            &self.streaming.streaming_text,
            if self.streaming.streaming_thinking.is_empty() {
                None
            } else {
                Some(self.streaming.streaming_thinking.clone())
            },
            !self.streaming.streaming_tool_calls.is_empty(),
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
            let hide = self.streaming.hide_thinking_block;
            let label = self.streaming.hidden_thinking_label.clone();
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
        while self.streaming.is_streaming {
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
                            self.ui.tui.force_render();
                        }
                        TerminalEvent::Paste(data) | TerminalEvent::Raw(data) => {
                            self.ui.tui.handle_raw_input(&data);
                            self.sync_bash_mode_from_editor();
                            self.refresh_ui();
                        }
                        TerminalEvent::Key(key) => {
                            // During streaming, Ctrl-C can interrupt
                            if key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                self.handle_ctrl_c();
                                if self.interaction.shutdown_requested {
                                    self.streaming.is_streaming = false;
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
                                    self.queues.steering_queue.push_back(text);
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
                                    self.queues.follow_up_queue.push_back(text);
                                    self.sync_pending_render_state();
                                    self.refresh_ui();
                                }
                            } else if key.code == KeyCode::F(10) {
                                self.handle_dequeue();
                                self.refresh_ui();
                            } else {
                                self.ui.tui.handle_key(&key);
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
                    if self.streaming.status_loader.is_some() {
                        self.refresh_ui();
                    }
                },
                // Both channels closed
                else => {
                    self.streaming.is_streaming = false;
                    break;
                }
            }
        }

        // Clean up agent events channel
        self.agent_events = None;
        Ok(())
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
                self.streaming.is_streaming = true;
                let loader_message = self
                    .streaming.pending_working_message
                    .take()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| self.streaming.default_working_message.to_string());
                self.streaming.status_loader = Some((StatusLoaderStyle::Accent, loader_message));
                self.streaming.streaming_text.clear();
                self.streaming.streaming_thinking.clear();
                self.streaming.streaming_tool_calls.clear();
                self.sync_streaming_assistant_component(None, None);
                self.refresh_ui();
            }
            AgentLoopEvent::TextDelta { text } => {
                self.streaming.streaming_text.push_str(&text);
                self.update_streaming_display();
            }
            AgentLoopEvent::ThinkingDelta { text } => {
                self.streaming.streaming_thinking.push_str(&text);
                // Match pi better: thinking may arrive before text, so create/update
                // the streaming assistant component even when text is still empty.
                self.update_streaming_display();
            }
            AgentLoopEvent::ToolCallStart { id, name } => {
                if !self.streaming.streaming_tool_calls.iter().any(|call| call.id == id) {
                    self.streaming.streaming_tool_calls.push(ToolCallContent {
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
                component.set_expanded(self.interaction.tool_output_expanded);
                self.render_state_mut().chat_items.push(ChatItem::ToolExecution(component.clone()));
                self.render_state_mut().pending_tools.insert(id, component);
                self.refresh_ui();
            }
            AgentLoopEvent::ToolCallDelta { id, args_delta } => {
                if let Some(call) = self.streaming.streaming_tool_calls.iter_mut().find(|call| call.id == id) {
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
                self.streaming.is_streaming = false;
                self.streaming.status_loader = None;
                self.streaming.streaming_text.clear();
                self.streaming.streaming_thinking.clear();
                self.streaming.streaming_tool_calls.clear();
                self.streaming.pending_working_message = None;
                self.render_state_mut().streaming_component = None;
                self.render_state_mut().streaming_message = None;
                self.render_state_mut().pending_tools.clear();
                self.check_auto_compaction();
                if let Some(queued) = self.queues.steering_queue.pop_front() {
                    self.render_cache.chat_lines.push(format!("queued(steer)> {queued}"));
                    self.streaming.pending_working_message = Some(queued);
                }
                self.rebuild_footer();
                self.refresh_ui();
            }
            AgentLoopEvent::Error { message } => {
                self.streaming.is_streaming = false;
                self.streaming.status_loader = None;
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
                self.streaming.streaming_text.clear();
                self.streaming.streaming_thinking.clear();
                self.streaming.streaming_tool_calls.clear();
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
        self.streaming.status_loader = None;
        self.render_state_mut().last_status = Some(format!("{yellow}[!]{reset} {dim}{message}{reset}"));
        self.render_cache.status_lines = vec![format!("{yellow}[!]{reset} {dim}{message}{reset}")];
    }

    pub(super) fn clear_status(&mut self) {
        self.render_cache.status_lines.clear();
    }

    pub(super) fn show_status(&mut self, message: impl Into<String>) {
        let message = message.into();
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        self.streaming.status_loader = None;
        self.render_state_mut().last_status = Some(format!("{dim}{message}{reset}"));
        self.render_cache.status_lines = vec![format!("{dim}{message}{reset}")];
    }
}