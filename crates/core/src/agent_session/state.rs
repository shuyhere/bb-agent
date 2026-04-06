use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::agent_session_extensions::SessionResourceBootstrap;

use super::config::AgentSessionConfig;
use super::events::{AgentSessionEventListener, Callback0};
use super::messages::CustomMessage;
use super::models::{ModelRef, ScopedModel, SessionStartEvent, ThinkingLevel};
use super::runtime::{
    AgentTool, BashExecutionMessage, ToolDefinition, ToolDefinitionEntry, ToolPromptGuideline,
    ToolPromptSnippet,
};

pub(super) struct AgentSessionState {
    pub(super) scoped_models: Vec<ScopedModel>,
    pub(super) unsubscribe_agent: Option<Callback0>,
    pub(super) event_listeners: Arc<Mutex<Vec<AgentSessionEventListener>>>,
    pub(super) agent_event_queue_depth: usize,
    pub(super) steering_messages: Vec<String>,
    pub(super) follow_up_messages: Vec<String>,
    pub(super) pending_next_turn_messages: Vec<CustomMessage>,
    pub(super) retry_in_flight: bool,
    pub(super) pending_bash_messages: Vec<BashExecutionMessage>,
    pub(super) turn_index: u64,
    pub(super) custom_tools: Vec<ToolDefinition>,
    pub(super) base_tool_definitions: Vec<ToolDefinition>,
    pub(super) cwd: PathBuf,
    pub(super) initial_active_tool_names: Option<Vec<String>>,
    pub(super) base_tools_override: Option<Vec<AgentTool>>,
    pub(super) resource_bootstrap: SessionResourceBootstrap,
    pub(super) session_start_event: SessionStartEvent,
    pub(super) tool_registry: Vec<AgentTool>,
    pub(super) tool_definitions: Vec<ToolDefinitionEntry>,
    pub(super) tool_prompt_snippets: Vec<ToolPromptSnippet>,
    pub(super) tool_prompt_guidelines: Vec<ToolPromptGuideline>,
    pub(super) base_system_prompt: String,
    pub(super) model: Option<ModelRef>,
    pub(super) thinking_level: ThinkingLevel,
    pub(super) is_streaming: bool,
}

impl AgentSessionState {
    pub(super) fn from_config(config: AgentSessionConfig) -> Self {
        Self {
            scoped_models: config.scoped_models,
            unsubscribe_agent: None,
            event_listeners: Arc::new(Mutex::new(Vec::new())),
            agent_event_queue_depth: 0,
            steering_messages: Vec::new(),
            follow_up_messages: Vec::new(),
            pending_next_turn_messages: Vec::new(),
            retry_in_flight: false,
            pending_bash_messages: Vec::new(),
            turn_index: 0,
            custom_tools: config.custom_tools,
            base_tool_definitions: Vec::new(),
            cwd: config.cwd,
            initial_active_tool_names: config.initial_active_tool_names,
            base_tools_override: config.base_tools_override,
            resource_bootstrap: config.resource_bootstrap,
            session_start_event: config
                .session_start_event
                .unwrap_or_else(SessionStartEvent::startup),
            tool_registry: Vec::new(),
            tool_definitions: Vec::new(),
            tool_prompt_snippets: Vec::new(),
            tool_prompt_guidelines: Vec::new(),
            base_system_prompt: String::new(),
            model: config.model,
            thinking_level: config.thinking_level,
            is_streaming: false,
        }
    }
}
