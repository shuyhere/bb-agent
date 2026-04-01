use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::config::AgentSessionConfig;
use super::events::{AgentSessionEventListener, Callback0};
use super::messages::CustomMessage;
use super::models::{ModelRef, ScopedModel, SessionStartEvent, ThinkingLevel};
use super::runtime::{
    AgentTool, BashExecutionMessage, RuntimeHandle, ToolDefinition, ToolDefinitionEntry,
    ToolPromptGuideline, ToolPromptSnippet,
};

#[allow(dead_code)]
pub(super) struct AgentSessionState {
    pub(super) agent: RuntimeHandle,
    pub(super) session_manager: RuntimeHandle,
    pub(super) settings_manager: RuntimeHandle,
    pub(super) scoped_models: Vec<ScopedModel>,
    pub(super) unsubscribe_agent: Option<Callback0>,
    pub(super) event_listeners: Arc<Mutex<Vec<AgentSessionEventListener>>>,
    pub(super) agent_event_queue_depth: usize,
    pub(super) steering_messages: Vec<String>,
    pub(super) follow_up_messages: Vec<String>,
    pub(super) pending_next_turn_messages: Vec<CustomMessage>,
    pub(super) compaction_in_flight: bool,
    pub(super) auto_compaction_in_flight: bool,
    pub(super) overflow_recovery_attempted: bool,
    pub(super) branch_summary_in_flight: bool,
    pub(super) retry_in_flight: bool,
    pub(super) retry_attempt: u32,
    pub(super) bash_in_flight: bool,
    pub(super) pending_bash_messages: Vec<BashExecutionMessage>,
    pub(super) extension_runner: Option<RuntimeHandle>,
    pub(super) turn_index: u64,
    pub(super) resource_loader: RuntimeHandle,
    pub(super) custom_tools: Vec<ToolDefinition>,
    pub(super) base_tool_definitions: Vec<ToolDefinition>,
    pub(super) cwd: PathBuf,
    pub(super) initial_active_tool_names: Option<Vec<String>>,
    pub(super) base_tools_override: Option<Vec<AgentTool>>,
    pub(super) session_start_event: SessionStartEvent,
    pub(super) model_registry: RuntimeHandle,
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
            agent: config.agent,
            session_manager: config.session_manager,
            settings_manager: config.settings_manager,
            scoped_models: config.scoped_models,
            unsubscribe_agent: None,
            event_listeners: Arc::new(Mutex::new(Vec::new())),
            agent_event_queue_depth: 0,
            steering_messages: Vec::new(),
            follow_up_messages: Vec::new(),
            pending_next_turn_messages: Vec::new(),
            compaction_in_flight: false,
            auto_compaction_in_flight: false,
            overflow_recovery_attempted: false,
            branch_summary_in_flight: false,
            retry_in_flight: false,
            retry_attempt: 0,
            bash_in_flight: false,
            pending_bash_messages: Vec::new(),
            extension_runner: None,
            turn_index: 0,
            resource_loader: config.resource_loader,
            custom_tools: config.custom_tools,
            base_tool_definitions: Vec::new(),
            cwd: config.cwd,
            initial_active_tool_names: config.initial_active_tool_names,
            base_tools_override: config.base_tools_override,
            session_start_event: config
                .session_start_event
                .unwrap_or_else(SessionStartEvent::startup),
            model_registry: config.model_registry,
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
