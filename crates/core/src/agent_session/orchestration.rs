use super::config::{PromptSource, StreamingBehavior};
use super::error::AgentSessionError;
use super::events::AgentSessionEvent;
use super::messages::{ImageContent, UserMessage, content_from_text_and_images};
use super::runtime::RuntimeBuildOptions;
use super::session::AgentSession;

impl AgentSession {
    pub(super) fn install_agent_subscription(&mut self) {
        // TODO: hook concrete runtime agent events.
        self.state.unsubscribe_agent = Some(Box::new(|| {}));
    }

    pub(super) fn build_runtime(&mut self, _options: RuntimeBuildOptions) {
        // TODO: port runtime tool / extension initialization.
    }

    pub(super) fn emit_ref(&self, event: &AgentSessionEvent) {
        let listeners = self
            .state
            .event_listeners
            .lock()
            .expect("agent session event listener mutex poisoned");
        for listener in listeners.iter() {
            listener(event);
        }
    }

    pub(super) fn emit_queue_update(&self) {
        self.emit_ref(&AgentSessionEvent::QueueUpdate {
            steering: self.state.steering_messages.clone(),
            follow_up: self.state.follow_up_messages.clone(),
        });
    }

    pub(super) fn try_execute_extension_command(&self, _text: &str) -> bool {
        // TODO: integrate extension command execution.
        false
    }

    pub(super) fn expand_skill_command(&self, text: String) -> String {
        // TODO: integrate resource loader based skill expansion.
        text
    }

    pub(super) fn expand_prompt_template(&self, text: String) -> String {
        // TODO: integrate prompt template expansion.
        text
    }

    pub(super) fn queue_steer(&mut self, text: String, images: Vec<ImageContent>) {
        self.state.steering_messages.push(text.clone());
        self.emit_queue_update();
        self.emit_ref(&AgentSessionEvent::UserMessageQueued {
            delivery: StreamingBehavior::Steer,
            message: UserMessage {
                content: content_from_text_and_images(text, images),
                source: PromptSource::Extension,
            },
        });
    }

    pub(super) fn queue_follow_up(&mut self, text: String, images: Vec<ImageContent>) {
        self.state.follow_up_messages.push(text.clone());
        self.emit_queue_update();
        self.emit_ref(&AgentSessionEvent::UserMessageQueued {
            delivery: StreamingBehavior::FollowUp,
            message: UserMessage {
                content: content_from_text_and_images(text, images),
                source: PromptSource::Extension,
            },
        });
    }

    pub(super) fn throw_if_extension_command(&self, _text: &str) -> Result<(), AgentSessionError> {
        // TODO: detect registered extension commands once the runtime extension
        // registry exists in bb-core. For now, unknown slash-prefixed commands
        // are treated like ordinary user text, matching pi's behavior for
        // non-extension commands.
        Ok(())
    }

    pub(super) fn flush_pending_bash_messages(&mut self) {
        if self.state.pending_bash_messages.is_empty() {
            return;
        }
        self.state.pending_bash_messages.clear();
        self.emit_ref(&AgentSessionEvent::BashMessagesFlushed);
    }

    pub(super) fn wait_for_retry(&mut self) {
        if self.state.retry_in_flight {
            self.state.retry_in_flight = false;
        }
    }
}
