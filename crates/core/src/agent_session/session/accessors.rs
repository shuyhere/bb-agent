use std::fmt;
use std::path::Path;

use super::super::config::AgentSessionConfig;
use super::super::events::{
    AgentSessionEvent, AgentSessionEventListener, ModelChangeSource, QueueState, SubscriptionHandle,
};
use super::super::models::{ModelRef, SessionStartEvent, ThinkingLevel};
use super::super::runtime::RuntimeBuildOptions;
use super::AgentSession;

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
            state: super::super::state::AgentSessionState::from_config(config),
        };

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
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let index = listeners.len();
        listeners.push(listener);
        SubscriptionHandle::new(index)
    }

    pub fn unsubscribe(&mut self, handle: SubscriptionHandle) -> bool {
        let mut listeners = self
            .state
            .event_listeners
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

    pub fn expand_input_text(&self, text: impl Into<String>) -> String {
        let text = text.into();
        self.expand_prompt_template(self.expand_skill_command(text))
    }

    pub fn is_extension_command_text(&self, text: &str) -> bool {
        self.throw_if_extension_command(text).is_err()
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
