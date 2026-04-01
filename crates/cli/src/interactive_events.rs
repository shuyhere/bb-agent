use std::collections::BTreeMap;

use serde_json::Value;

use super::components::assistant_message::{
    AssistantMessage, AssistantMessageComponent, AssistantMessageContent, AssistantStopReason,
};
use super::components::bash_execution::{BashExecutionComponent, TruncationResult};
use super::components::tool_execution::{
    ToolExecutionComponent, ToolExecutionOptions, ToolExecutionResult, ToolResultBlock,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueuedMessageMode {
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedMessage {
    pub text: String,
    pub mode: QueuedMessageMode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingMessages {
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
}

impl PendingMessages {
    pub fn is_empty(&self) -> bool {
        self.steering.is_empty() && self.follow_up.is_empty()
    }

    pub fn combined(&self) -> Vec<String> {
        self.steering
            .iter()
            .chain(self.follow_up.iter())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone)]
pub enum ChatItem {
    Spacer,
    UserMessage(String),
    AssistantMessage(AssistantMessageComponent),
    ToolExecution(ToolExecutionComponent),
    BashExecution(BashExecutionComponent),
    CustomMessage {
        custom_type: String,
        display: bool,
        text: String,
    },
    CompactionSummary(String),
    BranchSummary(String),
    PendingMessageLine(String),
}

#[derive(Debug, Clone)]
pub struct ToolCallContent {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub enum InteractiveMessage {
    User {
        text: String,
    },
    Assistant {
        message: AssistantMessage,
        tool_calls: Vec<ToolCallContent>,
    },
    ToolResult {
        tool_call_id: String,
        result: ToolExecutionResult,
    },
    BashExecution {
        command: String,
        output: Option<String>,
        exit_code: Option<i32>,
        cancelled: bool,
        truncated: bool,
        full_output_path: Option<String>,
        exclude_from_context: bool,
    },
    Custom {
        custom_type: String,
        text: String,
        display: bool,
    },
    CompactionSummary {
        summary: String,
    },
    BranchSummary {
        summary: String,
    },
}

#[derive(Debug, Clone)]
pub struct SessionContext {
    pub messages: Vec<InteractiveMessage>,
}

#[derive(Debug, Clone)]
pub enum InteractiveSessionEvent {
    AgentStart,
    QueueUpdate,
    MessageStart {
        message: InteractiveMessage,
    },
    MessageUpdate {
        message: InteractiveMessage,
    },
    MessageEnd {
        message: InteractiveMessage,
    },
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        args: Value,
    },
    ToolExecutionUpdate {
        tool_call_id: String,
        partial_result: ToolExecutionResult,
    },
    ToolExecutionEnd {
        tool_call_id: String,
        result: ToolExecutionResult,
        is_error: bool,
    },
    AgentEnd,
    CompactionStart,
    CompactionEnd {
        summary: Option<String>,
        error_message: Option<String>,
    },
    AutoRetryStart {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
    },
    AutoRetryEnd {
        success: bool,
        attempt: u32,
        final_error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct InteractiveRenderState {
    pub chat_items: Vec<ChatItem>,
    pub pending_items: Vec<ChatItem>,
    pub pending_tools: BTreeMap<String, ToolExecutionComponent>,
    pub streaming_component: Option<AssistantMessageComponent>,
    pub streaming_message: Option<AssistantMessage>,
    pub pending_working_message: Option<String>,
    pub retry_attempt: usize,
    pub tool_output_expanded: bool,
    pub hide_thinking_block: bool,
    pub hidden_thinking_label: String,
    pub show_images: bool,
    pub last_status: Option<String>,
}

impl Default for InteractiveRenderState {
    fn default() -> Self {
        Self {
            chat_items: Vec::new(),
            pending_items: Vec::new(),
            pending_tools: BTreeMap::new(),
            streaming_component: None,
            streaming_message: None,
            pending_working_message: None,
            retry_attempt: 0,
            tool_output_expanded: false,
            hide_thinking_block: false,
            hidden_thinking_label: "Thinking...".to_string(),
            show_images: true,
            last_status: None,
        }
    }
}

impl InteractiveRenderState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_event(&mut self, event: InteractiveSessionEvent, pending: &PendingMessages) {
        match event {
            InteractiveSessionEvent::AgentStart => {
                self.last_status = Some(
                    self.pending_working_message
                        .clone()
                        .unwrap_or_else(|| "Working...".to_string()),
                );
            }
            InteractiveSessionEvent::QueueUpdate => {
                self.update_pending_messages_display(pending);
            }
            InteractiveSessionEvent::MessageStart { message } => match message {
                InteractiveMessage::Custom { .. } => {
                    self.add_message_to_chat(message);
                }
                InteractiveMessage::User { .. } => {
                    self.add_message_to_chat(message);
                    self.update_pending_messages_display(pending);
                }
                InteractiveMessage::Assistant { message, .. } => {
                    let mut component = AssistantMessageComponent::new(
                        None::<AssistantMessage>,
                        self.hide_thinking_block,
                    );
                    component.set_hidden_thinking_label(self.hidden_thinking_label.clone());
                    component.update_content(message.clone());
                    self.streaming_component = Some(component);
                    self.streaming_message = Some(message.clone());
                    if let Some(component) = &self.streaming_component {
                        self.chat_items
                            .push(ChatItem::AssistantMessage(component.clone()));
                    }
                }
                _ => {
                    self.add_message_to_chat(message);
                }
            },
            InteractiveSessionEvent::MessageUpdate { message } => {
                if let InteractiveMessage::Assistant {
                    message,
                    tool_calls,
                } = message
                {
                    if let Some(component) = self.streaming_component.as_mut() {
                        component.update_content(message.clone());
                        self.streaming_message = Some(message.clone());

                        for tool_call in tool_calls {
                            if let Some(existing) = self.pending_tools.get_mut(&tool_call.id) {
                                existing.update_args(tool_call.arguments);
                            } else {
                                let mut component = self.new_tool_component(
                                    tool_call.name,
                                    tool_call.id.clone(),
                                    tool_call.arguments,
                                );
                                component.set_expanded(self.tool_output_expanded);
                                self.chat_items
                                    .push(ChatItem::ToolExecution(component.clone()));
                                self.pending_tools.insert(tool_call.id, component);
                            }
                        }
                    }
                }
            }
            InteractiveSessionEvent::MessageEnd { message } => {
                if let InteractiveMessage::Assistant { message, .. } = message {
                    let mut finalized = message.clone();
                    let mut error_message = None;

                    if matches!(finalized.stop_reason, Some(AssistantStopReason::Aborted)) {
                        let retry_attempt = self.retry_attempt;
                        let derived = if retry_attempt > 0 {
                            format!(
                                "Aborted after {retry_attempt} retry attempt{}",
                                if retry_attempt > 1 { "s" } else { "" }
                            )
                        } else {
                            "Operation aborted".to_string()
                        };
                        finalized.error_message = Some(derived.clone());
                        error_message = Some(derived);
                    }

                    if let Some(component) = self.streaming_component.as_mut() {
                        component.update_content(finalized.clone());
                    }

                    if matches!(
                        finalized.stop_reason,
                        Some(AssistantStopReason::Aborted | AssistantStopReason::Error)
                    ) {
                        let error_text = error_message
                            .or_else(|| finalized.error_message.clone())
                            .unwrap_or_else(|| "Error".to_string());
                        let result = ToolExecutionResult {
                            content: vec![ToolResultBlock {
                                r#type: "text".to_string(),
                                text: Some(error_text),
                                data: None,
                                mime_type: None,
                            }],
                            is_error: true,
                            details: None,
                        };
                        for component in self.pending_tools.values_mut() {
                            component.update_result(result.clone(), false);
                        }
                        self.pending_tools.clear();
                    } else {
                        for component in self.pending_tools.values_mut() {
                            component.set_args_complete();
                        }
                    }

                    self.streaming_component = None;
                    self.streaming_message = None;
                }
            }
            InteractiveSessionEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => {
                if let Some(component) = self.pending_tools.get_mut(&tool_call_id) {
                    component.mark_execution_started();
                } else {
                    let mut component =
                        self.new_tool_component(tool_name, tool_call_id.clone(), args);
                    component.set_expanded(self.tool_output_expanded);
                    component.mark_execution_started();
                    self.chat_items
                        .push(ChatItem::ToolExecution(component.clone()));
                    self.pending_tools.insert(tool_call_id, component);
                }
            }
            InteractiveSessionEvent::ToolExecutionUpdate {
                tool_call_id,
                partial_result,
            } => {
                if let Some(component) = self.pending_tools.get_mut(&tool_call_id) {
                    let mut partial = partial_result;
                    partial.is_error = false;
                    component.update_result(partial, true);
                }
            }
            InteractiveSessionEvent::ToolExecutionEnd {
                tool_call_id,
                mut result,
                is_error,
            } => {
                if let Some(component) = self.pending_tools.get_mut(&tool_call_id) {
                    result.is_error = is_error;
                    component.update_result(result, false);
                }
                self.pending_tools.remove(&tool_call_id);
            }
            InteractiveSessionEvent::AgentEnd => {
                self.streaming_component = None;
                self.streaming_message = None;
                self.pending_tools.clear();
                self.last_status = None;
            }
            InteractiveSessionEvent::CompactionStart => {
                self.last_status = Some("Compacting context...".to_string());
            }
            InteractiveSessionEvent::CompactionEnd {
                summary,
                error_message,
            } => {
                if let Some(summary) = summary {
                    self.chat_items.clear();
                    self.rebuild_chat_from_messages(&SessionContext {
                        messages: Vec::new(),
                    });
                    self.chat_items.push(ChatItem::CompactionSummary(summary));
                }
                if let Some(error_message) = error_message {
                    self.last_status = Some(error_message);
                }
            }
            InteractiveSessionEvent::AutoRetryStart {
                attempt,
                max_attempts,
                delay_ms,
            } => {
                self.retry_attempt = attempt as usize;
                self.last_status = Some(format!(
                    "Retrying ({attempt}/{max_attempts}) in {}s...",
                    delay_ms / 1000
                ));
            }
            InteractiveSessionEvent::AutoRetryEnd {
                success,
                attempt,
                final_error,
            } => {
                self.retry_attempt = attempt as usize;
                if success {
                    self.last_status = None;
                } else {
                    self.last_status = Some(format!(
                        "Retry failed after {attempt} attempts: {}",
                        final_error.unwrap_or_else(|| "Unknown error".to_string())
                    ));
                }
            }
        }
    }

    pub fn add_message_to_chat(&mut self, message: InteractiveMessage) {
        match message {
            InteractiveMessage::BashExecution {
                command,
                output,
                exit_code,
                cancelled,
                truncated,
                full_output_path,
                exclude_from_context,
            } => {
                let mut component = BashExecutionComponent::new(command, exclude_from_context);
                if let Some(output) = output {
                    component.append_output(&output);
                }
                component.set_complete(
                    exit_code,
                    cancelled,
                    truncated.then_some(TruncationResult {
                        content: String::new(),
                        truncated: true,
                        total_lines: 0,
                        total_bytes: 0,
                    }),
                    full_output_path,
                );
                self.chat_items.push(ChatItem::BashExecution(component));
            }
            InteractiveMessage::Custom {
                custom_type,
                text,
                display,
            } => {
                if display {
                    self.chat_items.push(ChatItem::CustomMessage {
                        custom_type,
                        display,
                        text,
                    });
                }
            }
            InteractiveMessage::CompactionSummary { summary } => {
                self.chat_items.push(ChatItem::Spacer);
                self.chat_items.push(ChatItem::CompactionSummary(summary));
            }
            InteractiveMessage::BranchSummary { summary } => {
                self.chat_items.push(ChatItem::Spacer);
                self.chat_items.push(ChatItem::BranchSummary(summary));
            }
            InteractiveMessage::User { text } => {
                self.chat_items.push(ChatItem::UserMessage(text));
            }
            InteractiveMessage::Assistant { message, .. } => {
                let mut component =
                    AssistantMessageComponent::new(Some(message), self.hide_thinking_block);
                component.set_hidden_thinking_label(self.hidden_thinking_label.clone());
                self.chat_items.push(ChatItem::AssistantMessage(component));
            }
            InteractiveMessage::ToolResult { .. } => {}
        }
    }

    pub fn render_session_context(&mut self, session_context: &SessionContext) {
        self.pending_tools.clear();

        for message in &session_context.messages {
            match message {
                InteractiveMessage::Assistant {
                    message,
                    tool_calls,
                } => {
                    self.add_message_to_chat(InteractiveMessage::Assistant {
                        message: message.clone(),
                        tool_calls: tool_calls.clone(),
                    });

                    for tool_call in tool_calls {
                        let mut component = self.new_tool_component(
                            tool_call.name.clone(),
                            tool_call.id.clone(),
                            tool_call.arguments.clone(),
                        );
                        component.set_expanded(self.tool_output_expanded);
                        self.chat_items
                            .push(ChatItem::ToolExecution(component.clone()));

                        if matches!(
                            message.stop_reason,
                            Some(AssistantStopReason::Aborted | AssistantStopReason::Error)
                        ) {
                            let error_message = if matches!(
                                message.stop_reason,
                                Some(AssistantStopReason::Aborted)
                            ) {
                                let retry_attempt = self.retry_attempt;
                                if retry_attempt > 0 {
                                    format!(
                                        "Aborted after {retry_attempt} retry attempt{}",
                                        if retry_attempt > 1 { "s" } else { "" }
                                    )
                                } else {
                                    "Operation aborted".to_string()
                                }
                            } else {
                                message
                                    .error_message
                                    .clone()
                                    .unwrap_or_else(|| "Error".to_string())
                            };
                            component.update_result(
                                ToolExecutionResult {
                                    content: vec![ToolResultBlock {
                                        r#type: "text".to_string(),
                                        text: Some(error_message),
                                        data: None,
                                        mime_type: None,
                                    }],
                                    is_error: true,
                                    details: None,
                                },
                                false,
                            );
                            if let Some(ChatItem::ToolExecution(last)) = self.chat_items.last_mut()
                            {
                                *last = component;
                            }
                        } else {
                            self.pending_tools.insert(tool_call.id.clone(), component);
                        }
                    }
                }
                InteractiveMessage::ToolResult {
                    tool_call_id,
                    result,
                } => {
                    if let Some(component) = self.pending_tools.get_mut(tool_call_id) {
                        component.update_result(result.clone(), false);
                    }
                    self.pending_tools.remove(tool_call_id);
                }
                other => self.add_message_to_chat(other.clone()),
            }
        }

        self.pending_tools.clear();
    }

    pub fn rebuild_chat_from_messages(&mut self, session_context: &SessionContext) {
        self.chat_items.clear();
        self.render_session_context(session_context);
    }

    pub fn update_pending_messages_display(&mut self, pending: &PendingMessages) {
        self.pending_items.clear();
        if pending.is_empty() {
            return;
        }

        self.pending_items.push(ChatItem::Spacer);
        for message in &pending.steering {
            self.pending_items
                .push(ChatItem::PendingMessageLine(format!("Steering: {message}")));
        }
        for message in &pending.follow_up {
            self.pending_items
                .push(ChatItem::PendingMessageLine(format!(
                    "Follow-up: {message}"
                )));
        }
        self.pending_items.push(ChatItem::PendingMessageLine(
            "↳ dequeue to edit all queued messages".to_string(),
        ));
    }

    pub fn queue_compaction_message(
        &mut self,
        compaction_queue: &mut Vec<QueuedMessage>,
        text: impl Into<String>,
        mode: QueuedMessageMode,
    ) {
        compaction_queue.push(QueuedMessage {
            text: text.into(),
            mode,
        });
        self.last_status = Some("Queued message for after compaction".to_string());
    }

    pub fn collect_pending_messages(
        steering_messages: &[String],
        follow_up_messages: &[String],
        compaction_queue: &[QueuedMessage],
    ) -> PendingMessages {
        let mut pending = PendingMessages {
            steering: steering_messages.to_vec(),
            follow_up: follow_up_messages.to_vec(),
        };

        for queued in compaction_queue {
            match queued.mode {
                QueuedMessageMode::Steer => pending.steering.push(queued.text.clone()),
                QueuedMessageMode::FollowUp => pending.follow_up.push(queued.text.clone()),
            }
        }

        pending
    }

    pub fn restore_queued_messages_to_editor(
        &mut self,
        pending: PendingMessages,
        current_text: Option<&str>,
    ) -> RestoreQueuedMessagesResult {
        let all_queued = pending.combined();
        if all_queued.is_empty() {
            self.update_pending_messages_display(&PendingMessages::default());
            return RestoreQueuedMessagesResult {
                restored_count: 0,
                editor_text: current_text.unwrap_or_default().to_string(),
            };
        }

        let queued_text = all_queued.join("\n\n");
        let current_text = current_text.unwrap_or_default();
        let editor_text = [queued_text.as_str(), current_text]
            .into_iter()
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");

        self.update_pending_messages_display(&PendingMessages::default());

        RestoreQueuedMessagesResult {
            restored_count: all_queued.len(),
            editor_text,
        }
    }

    fn new_tool_component(
        &self,
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
        args: Value,
    ) -> ToolExecutionComponent {
        ToolExecutionComponent::new(
            tool_name,
            tool_call_id,
            args,
            ToolExecutionOptions {
                show_images: self.show_images,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreQueuedMessagesResult {
    pub restored_count: usize,
    pub editor_text: String,
}

pub fn assistant_message_from_parts(
    text: impl Into<String>,
    thinking: Option<String>,
    has_tool_call: bool,
) -> AssistantMessage {
    let mut content = Vec::new();
    if let Some(thinking) = thinking {
        if !thinking.trim().is_empty() {
            content.push(AssistantMessageContent::Thinking(thinking));
        }
    }

    let text = text.into();
    if !text.trim().is_empty() {
        content.push(AssistantMessageContent::Text(text));
    }

    if has_tool_call {
        content.push(AssistantMessageContent::ToolCall);
    }

    AssistantMessage {
        content,
        stop_reason: Some(AssistantStopReason::Other),
        error_message: None,
    }
}
