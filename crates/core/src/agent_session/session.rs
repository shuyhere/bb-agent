use std::fmt;
use std::path::Path;

use super::config::{
    AgentSessionConfig, CustomMessageDelivery, PromptOptions, PromptSource,
    SendCustomMessageOptions, SendUserMessageOptions, StreamingBehavior,
};
use super::error::AgentSessionError;
use super::events::{
    AgentSessionEvent, AgentSessionEventListener, ModelChangeSource, QueueState, SubscriptionHandle,
};
use super::messages::{
    ContentPart, CustomMessage, ImageContent, SessionMessage, TextContent, UserMessage,
    UserMessageContent,
};
use super::models::{ModelRef, SessionStartEvent, ThinkingLevel};
use super::runtime::RuntimeBuildOptions;
use super::state::AgentSessionState;

/// Main session object for the bb-core public API.
///
/// This is a foundation port of pi's `AgentSession` shape. Runtime integrations
/// that depend on the concrete agent loop, session persistence, settings, model
/// registry, and extension system are intentionally left as TODO-safe hooks.
pub struct AgentSession {
    pub(super) state: AgentSessionState,
}

impl fmt::Debug for AgentSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentSession")
            .field("scoped_models", &self.state.scoped_models)
            .field(
                "agent_event_queue_depth",
                &self.state.agent_event_queue_depth,
            )
            .field("steering_messages", &self.state.steering_messages)
            .field("follow_up_messages", &self.state.follow_up_messages)
            .field(
                "pending_next_turn_messages",
                &self.state.pending_next_turn_messages,
            )
            .field("turn_index", &self.state.turn_index)
            .field("cwd", &self.state.cwd)
            .field("session_start_event", &self.state.session_start_event)
            .field("base_system_prompt", &self.state.base_system_prompt)
            .field("model", &self.state.model)
            .field("thinking_level", &self.state.thinking_level)
            .field("is_streaming", &self.state.is_streaming)
            .finish_non_exhaustive()
    }
}

impl AgentSession {
    pub fn new(config: AgentSessionConfig) -> Self {
        let mut session = Self {
            state: AgentSessionState::from_config(config),
        };

        // Faithful to pi's constructor shape: subscribe internal handlers first,
        // then build runtime state.
        session.install_agent_subscription();
        session.build_runtime(RuntimeBuildOptions {
            active_tool_names: session.state.initial_active_tool_names.clone(),
            include_all_extension_tools: true,
        });

        session
    }

    pub fn model(&self) -> Option<&ModelRef> {
        self.state.model.as_ref()
    }

    pub fn thinking_level(&self) -> ThinkingLevel {
        self.state.thinking_level
    }

    pub fn is_streaming(&self) -> bool {
        self.state.is_streaming
    }

    pub fn cwd(&self) -> &Path {
        &self.state.cwd
    }

    pub fn session_start_event(&self) -> &SessionStartEvent {
        &self.state.session_start_event
    }

    pub fn pending_message_count(&self) -> usize {
        self.state.steering_messages.len() + self.state.follow_up_messages.len()
    }

    pub fn get_steering_messages(&self) -> &[String] {
        &self.state.steering_messages
    }

    pub fn get_follow_up_messages(&self) -> &[String] {
        &self.state.follow_up_messages
    }

    pub fn subscribe(&mut self, listener: AgentSessionEventListener) -> SubscriptionHandle {
        let mut listeners = self
            .state
            .event_listeners
            .lock()
            .expect("agent session event listener mutex poisoned");
        let index = listeners.len();
        listeners.push(listener);
        SubscriptionHandle::new(index)
    }

    pub fn unsubscribe(&mut self, handle: SubscriptionHandle) -> bool {
        let mut listeners = self
            .state
            .event_listeners
            .lock()
            .expect("agent session event listener mutex poisoned");
        if let Some(slot) = listeners.get_mut(handle.index()) {
            *slot = Box::new(|_| {});
            true
        } else {
            false
        }
    }

    pub fn emit(&self, event: AgentSessionEvent) {
        self.emit_ref(&event);
    }

    pub fn prompt(
        &mut self,
        text: impl Into<String>,
        options: PromptOptions,
    ) -> Result<(), AgentSessionError> {
        let text = text.into();
        let expand_prompt_templates = options.expand_prompt_templates;

        if expand_prompt_templates && text.starts_with('/') {
            if self.try_execute_extension_command(&text) {
                return Ok(());
            }
        }

        let mut expanded_text = text;
        if expand_prompt_templates {
            expanded_text = self.expand_skill_command(expanded_text);
            expanded_text = self.expand_prompt_template(expanded_text);
        }

        if self.state.is_streaming {
            match options.streaming_behavior {
                Some(StreamingBehavior::FollowUp) => {
                    self.queue_follow_up(expanded_text, options.images);
                }
                Some(StreamingBehavior::Steer) => {
                    self.queue_steer(expanded_text, options.images);
                }
                None => {
                    return Err(AgentSessionError::AlreadyProcessing);
                }
            }
            return Ok(());
        }

        self.flush_pending_bash_messages();

        if self.state.model.is_none() {
            return Err(AgentSessionError::NoModelSelected);
        }

        let mut outgoing = Vec::new();
        let mut user_content = Vec::new();
        user_content.push(ContentPart::Text(TextContent {
            text: expanded_text,
        }));
        user_content.extend(options.images.into_iter().map(ContentPart::Image));
        outgoing.push(SessionMessage::User(UserMessage {
            content: user_content,
            source: options.source,
        }));

        outgoing.extend(
            self.state
                .pending_next_turn_messages
                .drain(..)
                .map(SessionMessage::Custom),
        );

        self.state.is_streaming = true;
        self.emit_ref(&AgentSessionEvent::PromptDispatched { messages: outgoing });
        self.wait_for_retry();
        self.state.is_streaming = false;
        Ok(())
    }

    pub fn steer(
        &mut self,
        text: impl Into<String>,
        images: Vec<ImageContent>,
    ) -> Result<(), AgentSessionError> {
        let text = text.into();
        if text.starts_with('/') {
            self.throw_if_extension_command(&text)?;
        }

        let expanded = self.expand_prompt_template(self.expand_skill_command(text));
        self.queue_steer(expanded, images);
        Ok(())
    }

    pub fn follow_up(
        &mut self,
        text: impl Into<String>,
        images: Vec<ImageContent>,
    ) -> Result<(), AgentSessionError> {
        let text = text.into();
        if text.starts_with('/') {
            self.throw_if_extension_command(&text)?;
        }

        let expanded = self.expand_prompt_template(self.expand_skill_command(text));
        self.queue_follow_up(expanded, images);
        Ok(())
    }

    pub fn send_custom_message(
        &mut self,
        message: CustomMessage,
        options: SendCustomMessageOptions,
    ) -> Result<(), AgentSessionError> {
        if matches!(options.deliver_as, Some(CustomMessageDelivery::NextTurn)) {
            self.state.pending_next_turn_messages.push(message);
            return Ok(());
        }

        if self.state.is_streaming {
            match options.deliver_as.unwrap_or(CustomMessageDelivery::Steer) {
                CustomMessageDelivery::Steer => {
                    self.emit_ref(&AgentSessionEvent::CustomMessageQueued {
                        delivery: CustomMessageDelivery::Steer,
                        message: message.clone(),
                    });
                }
                CustomMessageDelivery::FollowUp => {
                    self.emit_ref(&AgentSessionEvent::CustomMessageQueued {
                        delivery: CustomMessageDelivery::FollowUp,
                        message: message.clone(),
                    });
                }
                CustomMessageDelivery::NextTurn => {}
            }
            return Ok(());
        }

        if options.trigger_turn {
            self.emit_ref(&AgentSessionEvent::PromptDispatched {
                messages: vec![SessionMessage::Custom(message)],
            });
            return Ok(());
        }

        self.emit_ref(&AgentSessionEvent::MessageStart {
            message: SessionMessage::Custom(message.clone()),
        });
        self.emit_ref(&AgentSessionEvent::MessageEnd {
            message: SessionMessage::Custom(message),
        });
        Ok(())
    }

    pub fn send_user_message(
        &mut self,
        content: UserMessageContent,
        options: SendUserMessageOptions,
    ) -> Result<(), AgentSessionError> {
        let (text, images) = content.into_text_and_images();
        self.prompt(
            text,
            PromptOptions {
                expand_prompt_templates: false,
                streaming_behavior: options.deliver_as,
                images,
                source: PromptSource::Extension,
            },
        )
    }

    pub fn clear_queue(&mut self) -> QueueState {
        let steering = std::mem::take(&mut self.state.steering_messages);
        let follow_up = std::mem::take(&mut self.state.follow_up_messages);
        let state = QueueState {
            steering,
            follow_up,
        };
        self.emit_queue_update();
        state
    }

    pub fn set_model(&mut self, model: ModelRef) {
        self.state.model = Some(model.clone());
        self.emit_ref(&AgentSessionEvent::ModelChanged {
            model,
            source: ModelChangeSource::Set,
        });
    }

    pub fn set_thinking_level(&mut self, level: ThinkingLevel) {
        self.state.thinking_level = level;
        self.emit_ref(&AgentSessionEvent::ThinkingLevelChanged { level });
    }
}
