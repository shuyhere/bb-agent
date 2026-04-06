use super::super::config::{
    CustomMessageDelivery, PromptOptions, PromptSource, SendCustomMessageOptions,
    SendUserMessageOptions, StreamingBehavior,
};
use super::super::error::AgentSessionError;
use super::super::events::AgentSessionEvent;
use super::super::messages::{
    ContentPart, CustomMessage, ImageContent, SessionMessage, TextContent, UserMessage,
    UserMessageContent,
};
use super::AgentSession;

impl AgentSession {
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
            self.throw_if_extension_command(&text)?;
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
}
