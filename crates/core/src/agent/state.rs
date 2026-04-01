use std::collections::HashSet;

use super::data::{AgentMessage, AgentModel, AgentTool, ThinkingLevel};

#[derive(Clone, Debug)]
pub struct AgentState {
    pub system_prompt: String,
    pub model: AgentModel,
    pub thinking_level: ThinkingLevel,
    pub tools: Vec<AgentTool>,
    pub messages: Vec<AgentMessage>,
    pub is_streaming: bool,
    pub streaming_message: Option<AgentMessage>,
    pub pending_tool_calls: HashSet<String>,
    pub error_message: Option<String>,
}

impl Default for AgentState {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            model: AgentModel::default(),
            thinking_level: ThinkingLevel::Off,
            tools: Vec::new(),
            messages: Vec::new(),
            is_streaming: false,
            streaming_message: None,
            pending_tool_calls: HashSet::new(),
            error_message: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AgentStateInit {
    pub system_prompt: Option<String>,
    pub model: Option<AgentModel>,
    pub thinking_level: Option<ThinkingLevel>,
    pub tools: Option<Vec<AgentTool>>,
    pub messages: Option<Vec<AgentMessage>>,
}

impl AgentState {
    pub fn from_init(initial: AgentStateInit) -> Self {
        Self {
            system_prompt: initial.system_prompt.unwrap_or_default(),
            model: initial.model.unwrap_or_default(),
            thinking_level: initial.thinking_level.unwrap_or_default(),
            tools: initial.tools.unwrap_or_default(),
            messages: initial.messages.unwrap_or_default(),
            is_streaming: false,
            streaming_message: None,
            pending_tool_calls: HashSet::new(),
            error_message: None,
        }
    }
}
