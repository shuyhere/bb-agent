mod bindings;
mod registry;

pub use registry::create_all_tool_definitions;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::models::ModelRegistryState;
use super::resources::ResourceLoaderState;
use super::types::{
    AgentTool, CommandContextAction, DiscoveredResourcePath, ErrorListener, ExtensionBindings,
    ExtensionCoreBindings, ExtensionErrorEvent, ExtensionRuntimeState, LoadedExtension,
    ModelDescriptor, PromptTemplateInfo, RefreshToolRegistryOptions, RegisteredCommand,
    RegisteredTool, ResourceExtensionPaths, ResourceOrigin, ResourcePathEntry,
    ResourcePathMetadata, ResourceScope, ResourcesDiscoverResult, RuntimeBuildOptions,
    RuntimeFlagValue, SessionSettings, SessionStartEvent, SessionStartReason, ShutdownHandler,
    SlashCommandInfo, SlashCommandSource, SourceInfo, ToolDefinition, ToolDefinitionEntry,
    UiContextBinding,
};

#[derive(Clone, Default)]
pub struct ExtensionRunnerState {
    pub extensions: Vec<LoadedExtension>,
    pub runtime: ExtensionRuntimeState,
    pub cwd: PathBuf,
    pub registered_commands: Vec<RegisteredCommand>,
    pub registered_tools: Vec<RegisteredTool>,
    pub resources_discover: BTreeMap<SessionStartReason, ResourcesDiscoverResult>,
    pub has_resources_discover_handler: bool,
    pub ui_context: Option<UiContextBinding>,
    pub command_context_actions: Vec<CommandContextAction>,
    pub session_start_events: Vec<SessionStartEvent>,
    pub shutdown_emitted: bool,
    pub error_listener: Option<ErrorListener>,
}

impl ExtensionRunnerState {
    pub fn new(
        extensions: Vec<LoadedExtension>,
        runtime: ExtensionRuntimeState,
        cwd: PathBuf,
    ) -> Self {
        Self {
            extensions,
            runtime,
            cwd,
            ..Self::default()
        }
    }

    pub fn has_handlers(&self, event: &str) -> bool {
        event == "resources_discover" && self.has_resources_discover_handler
    }

    pub fn emit_resources_discover(
        &self,
        _cwd: &Path,
        reason: SessionStartReason,
    ) -> ResourcesDiscoverResult {
        self.resources_discover
            .get(&reason)
            .cloned()
            .unwrap_or_default()
    }

    pub fn emit_session_start(&mut self, event: SessionStartEvent) {
        self.session_start_events.push(event);
    }

    pub fn emit_session_shutdown(&mut self) {
        self.shutdown_emitted = true;
    }

    pub fn set_ui_context(&mut self, ui_context: Option<UiContextBinding>) {
        self.ui_context = ui_context;
    }

    pub fn bind_command_context(&mut self, actions: Vec<CommandContextAction>) {
        self.command_context_actions = actions;
    }

    pub fn on_error(&mut self, listener: Option<ErrorListener>) {
        self.error_listener = listener;
    }

    pub fn emit_error(&self, error: ExtensionErrorEvent) {
        if let Some(listener) = &self.error_listener {
            listener(error);
        }
    }

    pub fn get_registered_commands(&self) -> Vec<RegisteredCommand> {
        self.registered_commands.clone()
    }

    pub fn get_all_registered_tools(&self) -> Vec<RegisteredTool> {
        self.registered_tools.clone()
    }

    pub fn get_flag_values(&self) -> BTreeMap<String, RuntimeFlagValue> {
        self.runtime.flag_values.clone()
    }
}

#[derive(Clone)]
pub struct AgentSessionExtensions {
    pub cwd: PathBuf,
    pub session_start_event: SessionStartEvent,
    pub extension_ui_context: Option<UiContextBinding>,
    pub extension_command_context_actions: Vec<CommandContextAction>,
    pub extension_shutdown_handler: Option<ShutdownHandler>,
    pub extension_error_listener: Option<ErrorListener>,
    pub resource_loader: ResourceLoaderState,
    pub model_registry: ModelRegistryState,
    pub extension_runner: Option<ExtensionRunnerState>,
    pub base_tool_definitions: BTreeMap<String, ToolDefinition>,
    pub tool_definitions: BTreeMap<String, ToolDefinitionEntry>,
    pub tool_prompt_snippets: BTreeMap<String, String>,
    pub tool_prompt_guidelines: BTreeMap<String, Vec<String>>,
    pub tool_registry: BTreeMap<String, AgentTool>,
    pub custom_tools: Vec<RegisteredTool>,
    pub base_tools_override: Option<BTreeMap<String, AgentTool>>,
    pub prompt_templates: Vec<PromptTemplateInfo>,
    pub active_tool_names: Vec<String>,
    pub base_system_prompt: String,
    pub system_prompt: String,
    pub model: Option<ModelDescriptor>,
    pub settings: SessionSettings,
}
