use std::path::PathBuf;

use crate::agent_session_extensions::SessionResourceBootstrap;

use super::messages::ImageContent;
use super::models::{ModelRef, ScopedModel, SessionStartEvent, ThinkingLevel};
use super::runtime::{AgentTool, RuntimeHandle, ToolDefinition};

#[derive(Debug, Clone)]
pub struct AgentSessionConfig {
    pub agent: RuntimeHandle,
    pub session_manager: RuntimeHandle,
    pub settings_manager: RuntimeHandle,
    pub scoped_models: Vec<ScopedModel>,
    pub resource_loader: RuntimeHandle,
    pub custom_tools: Vec<ToolDefinition>,
    pub cwd: PathBuf,
    pub model_registry: RuntimeHandle,
    pub initial_active_tool_names: Option<Vec<String>>,
    pub base_tools_override: Option<Vec<AgentTool>>,
    pub resource_bootstrap: SessionResourceBootstrap,
    pub session_start_event: Option<SessionStartEvent>,
    pub model: Option<ModelRef>,
    pub thinking_level: ThinkingLevel,
}

impl Default for AgentSessionConfig {
    fn default() -> Self {
        Self {
            agent: RuntimeHandle::placeholder("agent"),
            session_manager: RuntimeHandle::placeholder("session_manager"),
            settings_manager: RuntimeHandle::placeholder("settings_manager"),
            scoped_models: Vec::new(),
            resource_loader: RuntimeHandle::placeholder("resource_loader"),
            custom_tools: Vec::new(),
            cwd: PathBuf::new(),
            model_registry: RuntimeHandle::placeholder("model_registry"),
            initial_active_tool_names: None,
            base_tools_override: None,
            resource_bootstrap: SessionResourceBootstrap::default(),
            session_start_event: None,
            model: None,
            thinking_level: ThinkingLevel::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptOptions {
    pub expand_prompt_templates: bool,
    pub streaming_behavior: Option<StreamingBehavior>,
    pub images: Vec<ImageContent>,
    pub source: PromptSource,
}

impl Default for PromptOptions {
    fn default() -> Self {
        Self {
            expand_prompt_templates: true,
            streaming_behavior: None,
            images: Vec::new(),
            source: PromptSource::Interactive,
        }
    }
}

impl PromptOptions {
    pub fn expanded() -> Self {
        Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamingBehavior {
    Steer,
    FollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomMessageDelivery {
    Steer,
    FollowUp,
    NextTurn,
}

#[derive(Debug, Clone, Default)]
pub struct SendCustomMessageOptions {
    pub trigger_turn: bool,
    pub deliver_as: Option<CustomMessageDelivery>,
}

#[derive(Debug, Clone, Default)]
pub struct SendUserMessageOptions {
    pub deliver_as: Option<StreamingBehavior>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptSource {
    #[default]
    Interactive,
    Extension,
}
