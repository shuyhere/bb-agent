use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type ShutdownHandler = Arc<dyn Fn() + Send + Sync + 'static>;
pub type ErrorListener = Arc<dyn Fn(ExtensionErrorEvent) + Send + Sync + 'static>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UiContextBinding {
    pub values: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandContextAction {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Default)]
pub struct ExtensionBindings {
    pub ui_context: Option<UiContextBinding>,
    pub command_context_actions: Option<Vec<CommandContextAction>>,
    pub shutdown_handler: Option<ShutdownHandler>,
    pub on_error: Option<ErrorListener>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SessionStartReason {
    Startup,
    Reload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionStartEvent {
    pub reason: SessionStartReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionErrorEvent {
    pub extension_path: String,
    pub event: String,
    pub error: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiscoveredResourcePath {
    pub path: PathBuf,
    pub extension_path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourcesDiscoverResult {
    pub skill_paths: Vec<DiscoveredResourcePath>,
    pub prompt_paths: Vec<DiscoveredResourcePath>,
    pub theme_paths: Vec<DiscoveredResourcePath>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourcePathMetadata {
    pub source: String,
    pub scope: ResourceScope,
    pub origin: ResourceOrigin,
    pub base_dir: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceScope {
    Temporary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceOrigin {
    TopLevel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourcePathEntry {
    pub path: PathBuf,
    pub metadata: ResourcePathMetadata,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceExtensionPaths {
    pub skill_paths: Vec<ResourcePathEntry>,
    pub prompt_paths: Vec<ResourcePathEntry>,
    pub theme_paths: Vec<ResourcePathEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceInfo {
    pub path: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlashCommandInfo {
    pub name: String,
    pub description: String,
    pub source: SlashCommandSource,
    pub source_info: SourceInfo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlashCommandSource {
    Extension,
    Prompt,
    Skill,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptTemplateInfo {
    pub name: String,
    pub description: String,
    pub source_info: SourceInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub source_info: SourceInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SkillCatalog {
    pub skills: Vec<SkillInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegisteredCommand {
    pub invocation_name: String,
    pub description: String,
    pub source_info: SourceInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub prompt_snippet: Option<String>,
    pub prompt_guidelines: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentTool {
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolDefinitionEntry {
    pub definition: ToolDefinition,
    pub source_info: SourceInfo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegisteredTool {
    pub definition: ToolDefinition,
    pub source_info: SourceInfo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeFlagValue {
    Bool(bool),
    String(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionRuntimeState {
    pub flag_values: BTreeMap<String, RuntimeFlagValue>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadedExtension {
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionsResult {
    pub extensions: Vec<LoadedExtension>,
    pub runtime: ExtensionRuntimeState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceLoaderState {
    pub extension_paths: ResourceExtensionPaths,
    pub extensions_result: ExtensionsResult,
    pub skills: SkillCatalog,
}

impl ResourceLoaderState {
    pub fn extend_resources(&mut self, paths: ResourceExtensionPaths) {
        self.extension_paths.skill_paths.extend(paths.skill_paths);
        self.extension_paths.prompt_paths.extend(paths.prompt_paths);
        self.extension_paths.theme_paths.extend(paths.theme_paths);
    }

    pub fn reload(&mut self) {
        self.extension_paths = ResourceExtensionPaths::default();
    }

    pub fn get_extensions(&self) -> ExtensionsResult {
        self.extensions_result.clone()
    }

    pub fn get_skills(&self) -> SkillCatalog {
        self.skills.clone()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelDescriptor {
    pub provider: String,
    pub id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderConfig {
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelRegistryState {
    pub models: BTreeMap<(String, String), ModelDescriptor>,
    pub providers: BTreeMap<String, ProviderConfig>,
}

impl ModelRegistryState {
    pub fn find(&self, provider: &str, id: &str) -> Option<ModelDescriptor> {
        self.models
            .get(&(provider.to_owned(), id.to_owned()))
            .cloned()
    }

    pub fn register_provider(&mut self, name: impl Into<String>, config: ProviderConfig) {
        self.providers.insert(name.into(), config);
    }

    pub fn unregister_provider(&mut self, name: &str) {
        self.providers.remove(name);
    }
}

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
    pub fn new(extensions: Vec<LoadedExtension>, runtime: ExtensionRuntimeState, cwd: PathBuf) -> Self {
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

    pub fn emit_resources_discover(&self, _cwd: &Path, reason: SessionStartReason) -> ResourcesDiscoverResult {
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionSettings {
    pub image_auto_resize: bool,
    pub shell_command_prefix: Option<String>,
}

impl SessionSettings {
    pub fn reload(&mut self) {}
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefreshToolRegistryOptions {
    pub active_tool_names: Option<Vec<String>>,
    pub include_all_extension_tools: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuntimeBuildOptions {
    pub active_tool_names: Option<Vec<String>>,
    pub flag_values: BTreeMap<String, RuntimeFlagValue>,
    pub include_all_extension_tools: bool,
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

impl AgentSessionExtensions {
    pub fn bind_extensions(&mut self, bindings: ExtensionBindings) {
        if let Some(ui_context) = bindings.ui_context {
            self.extension_ui_context = Some(ui_context);
        }
        if let Some(actions) = bindings.command_context_actions {
            self.extension_command_context_actions = actions;
        }
        if let Some(shutdown_handler) = bindings.shutdown_handler {
            self.extension_shutdown_handler = Some(shutdown_handler);
        }
        if let Some(on_error) = bindings.on_error {
            self.extension_error_listener = Some(on_error);
        }

        if self.extension_runner.is_some() {
            self.apply_extension_bindings();
            let event = self.session_start_event;
            if let Some(runner) = self.extension_runner.as_mut() {
                runner.emit_session_start(event);
            }
            let reason = match event.reason {
                SessionStartReason::Reload => SessionStartReason::Reload,
                SessionStartReason::Startup => SessionStartReason::Startup,
            };
            self.extend_resources_from_extensions(reason);
        }
    }

    pub fn extend_resources_from_extensions(&mut self, reason: SessionStartReason) {
        let Some(runner) = self.extension_runner.as_ref() else {
            return;
        };
        if !runner.has_handlers("resources_discover") {
            return;
        }

        let discovered = runner.emit_resources_discover(&self.cwd, reason);
        if discovered.skill_paths.is_empty()
            && discovered.prompt_paths.is_empty()
            && discovered.theme_paths.is_empty()
        {
            return;
        }

        let extension_paths = ResourceExtensionPaths {
            skill_paths: self.build_extension_resource_paths(&discovered.skill_paths),
            prompt_paths: self.build_extension_resource_paths(&discovered.prompt_paths),
            theme_paths: self.build_extension_resource_paths(&discovered.theme_paths),
        };

        self.resource_loader.extend_resources(extension_paths);
        self.base_system_prompt = self.rebuild_system_prompt(&self.get_active_tool_names());
        self.system_prompt = self.base_system_prompt.clone();
    }

    pub fn build_extension_resource_paths(
        &self,
        entries: &[DiscoveredResourcePath],
    ) -> Vec<ResourcePathEntry> {
        entries
            .iter()
            .map(|entry| {
                let source = self.get_extension_source_label(&entry.extension_path);
                let base_dir = if entry.extension_path.starts_with('<') {
                    None
                } else {
                    Path::new(&entry.extension_path).parent().map(Path::to_path_buf)
                };
                ResourcePathEntry {
                    path: entry.path.clone(),
                    metadata: ResourcePathMetadata {
                        source,
                        scope: ResourceScope::Temporary,
                        origin: ResourceOrigin::TopLevel,
                        base_dir,
                    },
                }
            })
            .collect()
    }

    pub fn get_extension_source_label(&self, extension_path: &str) -> String {
        if extension_path.starts_with('<') {
            return format!("extension:{}", extension_path.trim_matches(&['<', '>'][..]));
        }

        let stem = Path::new(extension_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(extension_path);
        format!("extension:{stem}")
    }

    pub fn apply_extension_bindings(&mut self) {
        if let Some(runner) = self.extension_runner.as_mut() {
            runner.set_ui_context(self.extension_ui_context.clone());
            runner.bind_command_context(self.extension_command_context_actions.clone());
            runner.on_error(self.extension_error_listener.clone());
        }
    }

    pub fn refresh_current_model_from_registry(&mut self) {
        let Some(current_model) = self.model.clone() else {
            return;
        };

        let refreshed = self
            .model_registry
            .find(&current_model.provider, &current_model.id);
        if let Some(refreshed_model) = refreshed {
            if refreshed_model != current_model {
                self.model = Some(refreshed_model);
            }
        }
    }

    pub fn bind_extension_core(&self) -> ExtensionCoreBindings {
        ExtensionCoreBindings {
            commands: self.get_commands(),
            active_tools: self.get_active_tool_names(),
            all_tools: self.get_all_tools(),
            model: self.model.clone(),
            system_prompt: self.system_prompt.clone(),
        }
    }

    pub fn refresh_tool_registry(&mut self, options: RefreshToolRegistryOptions) {
        let previous_registry_names: BTreeSet<String> = self.tool_registry.keys().cloned().collect();
        let previous_active_tool_names = self.get_active_tool_names();

        let mut all_custom_tools = self
            .extension_runner
            .as_ref()
            .map(|runner| runner.get_all_registered_tools())
            .unwrap_or_default();
        all_custom_tools.extend(self.custom_tools.clone());

        let mut definition_registry: BTreeMap<String, ToolDefinitionEntry> = self
            .base_tool_definitions
            .iter()
            .map(|(name, definition)| {
                (
                    name.clone(),
                    ToolDefinitionEntry {
                        definition: definition.clone(),
                        source_info: SourceInfo {
                            path: format!("<builtin:{name}>"),
                            source: "builtin".to_owned(),
                        },
                    },
                )
            })
            .collect();

        for tool in &all_custom_tools {
            definition_registry.insert(
                tool.definition.name.clone(),
                ToolDefinitionEntry {
                    definition: tool.definition.clone(),
                    source_info: tool.source_info.clone(),
                },
            );
        }

        self.tool_definitions = definition_registry.clone();
        self.tool_prompt_snippets = definition_registry
            .values()
            .filter_map(|entry| {
                self.normalize_prompt_snippet(entry.definition.prompt_snippet.clone())
                    .map(|snippet| (entry.definition.name.clone(), snippet))
            })
            .collect();
        self.tool_prompt_guidelines = definition_registry
            .values()
            .filter_map(|entry| {
                let guidelines = self.normalize_prompt_guidelines(entry.definition.prompt_guidelines.clone());
                (!guidelines.is_empty()).then(|| (entry.definition.name.clone(), guidelines))
            })
            .collect();

        let mut tool_registry: BTreeMap<String, AgentTool> = self
            .base_tool_definitions
            .values()
            .map(|definition| {
                (
                    definition.name.clone(),
                    AgentTool {
                        name: definition.name.clone(),
                    },
                )
            })
            .collect();

        let wrapped_extension_tools: Vec<AgentTool> = all_custom_tools
            .iter()
            .map(|tool| AgentTool {
                name: tool.definition.name.clone(),
            })
            .collect();
        for tool in &wrapped_extension_tools {
            tool_registry.insert(tool.name.clone(), tool.clone());
        }
        self.tool_registry = tool_registry;

        let mut next_active_tool_names = options
            .active_tool_names
            .unwrap_or_else(|| previous_active_tool_names.clone());

        if options.include_all_extension_tools {
            next_active_tool_names.extend(wrapped_extension_tools.iter().map(|tool| tool.name.clone()));
        } else if next_active_tool_names == previous_active_tool_names {
            for tool_name in self.tool_registry.keys() {
                if !previous_registry_names.contains(tool_name) {
                    next_active_tool_names.push(tool_name.clone());
                }
            }
        }

        self.set_active_tools_by_name(unique_preserving_order(next_active_tool_names));
    }

    pub fn build_runtime(&mut self, options: RuntimeBuildOptions) {
        let base_tool_definitions = if let Some(overrides) = &self.base_tools_override {
            overrides
                .values()
                .map(|tool| {
                    (
                        tool.name.clone(),
                        ToolDefinition {
                            name: tool.name.clone(),
                            prompt_snippet: None,
                            prompt_guidelines: Vec::new(),
                        },
                    )
                })
                .collect()
        } else {
            create_all_tool_definitions()
        };
        self.base_tool_definitions = base_tool_definitions;

        let mut extensions_result = self.resource_loader.get_extensions();
        for (name, value) in options.flag_values {
            extensions_result.runtime.flag_values.insert(name, value);
        }

        let has_extensions = !extensions_result.extensions.is_empty();
        let has_custom_tools = !self.custom_tools.is_empty();
        self.extension_runner = if has_extensions || has_custom_tools {
            Some(ExtensionRunnerState::new(
                extensions_result.extensions,
                extensions_result.runtime,
                self.cwd.clone(),
            ))
        } else {
            None
        };
        if self.extension_runner.is_some() {
            let _ = self.bind_extension_core();
            self.apply_extension_bindings();
        }

        let default_active_tool_names = if let Some(overrides) = &self.base_tools_override {
            overrides.keys().cloned().collect()
        } else {
            vec![
                "read".to_owned(),
                "bash".to_owned(),
                "edit".to_owned(),
                "write".to_owned(),
            ]
        };
        let base_active_tool_names = options.active_tool_names.unwrap_or(default_active_tool_names);
        self.refresh_tool_registry(RefreshToolRegistryOptions {
            active_tool_names: Some(base_active_tool_names),
            include_all_extension_tools: options.include_all_extension_tools,
        });
    }

    pub fn reload(&mut self) {
        let previous_flag_values = self
            .extension_runner
            .as_ref()
            .map(|runner| runner.get_flag_values())
            .unwrap_or_default();

        if let Some(runner) = self.extension_runner.as_mut() {
            runner.emit_session_shutdown();
        }
        self.settings.reload();
        self.resource_loader.reload();
        self.build_runtime(RuntimeBuildOptions {
            active_tool_names: Some(self.get_active_tool_names()),
            flag_values: previous_flag_values,
            include_all_extension_tools: true,
        });

        let has_bindings = self.extension_ui_context.is_some()
            || !self.extension_command_context_actions.is_empty()
            || self.extension_shutdown_handler.is_some()
            || self.extension_error_listener.is_some();
        if has_bindings {
            if let Some(runner) = self.extension_runner.as_mut() {
                runner.emit_session_start(SessionStartEvent {
                    reason: SessionStartReason::Reload,
                });
            }
            self.extend_resources_from_extensions(SessionStartReason::Reload);
        }
    }

    pub fn get_commands(&self) -> Vec<SlashCommandInfo> {
        let extension_commands = self
            .extension_runner
            .as_ref()
            .map(|runner| runner.get_registered_commands())
            .unwrap_or_default()
            .into_iter()
            .map(|command| SlashCommandInfo {
                name: command.invocation_name,
                description: command.description,
                source: SlashCommandSource::Extension,
                source_info: command.source_info,
            });

        let template_commands = self.prompt_templates.iter().cloned().map(|template| SlashCommandInfo {
            name: template.name,
            description: template.description,
            source: SlashCommandSource::Prompt,
            source_info: template.source_info,
        });

        let skill_commands = self
            .resource_loader
            .get_skills()
            .skills
            .into_iter()
            .map(|skill| SlashCommandInfo {
                name: format!("skill:{}", skill.name),
                description: skill.description,
                source: SlashCommandSource::Skill,
                source_info: skill.source_info,
            });

        extension_commands
            .chain(template_commands)
            .chain(skill_commands)
            .collect()
    }

    pub fn get_active_tool_names(&self) -> Vec<String> {
        self.active_tool_names.clone()
    }

    pub fn set_active_tools_by_name(&mut self, tool_names: Vec<String>) {
        self.active_tool_names = tool_names;
    }

    pub fn get_all_tools(&self) -> Vec<String> {
        self.tool_registry.keys().cloned().collect()
    }

    fn rebuild_system_prompt(&self, active_tool_names: &[String]) -> String {
        format!("TODO: rebuild system prompt with tools: {}", active_tool_names.join(", "))
    }

    fn normalize_prompt_snippet(&self, snippet: Option<String>) -> Option<String> {
        snippet.and_then(|value| {
            let trimmed = value.trim().to_owned();
            (!trimmed.is_empty()).then_some(trimmed)
        })
    }

    fn normalize_prompt_guidelines(&self, guidelines: Vec<String>) -> Vec<String> {
        guidelines
            .into_iter()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionCoreBindings {
    pub commands: Vec<SlashCommandInfo>,
    pub active_tools: Vec<String>,
    pub all_tools: Vec<String>,
    pub model: Option<ModelDescriptor>,
    pub system_prompt: String,
}

pub fn create_all_tool_definitions() -> BTreeMap<String, ToolDefinition> {
    ["read", "bash", "edit", "write"]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                ToolDefinition {
                    name: name.to_owned(),
                    prompt_snippet: None,
                    prompt_guidelines: Vec::new(),
                },
            )
        })
        .collect()
}

fn unique_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for value in values {
        if seen.insert(value.clone()) {
            deduped.push(value);
        }
    }
    deduped
}
