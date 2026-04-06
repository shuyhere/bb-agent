use std::collections::BTreeMap;
use std::path::PathBuf;
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
    User,
    Project,
    Temporary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceOrigin {
    TopLevel,
    Package,
    Settings,
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

impl ResourceExtensionPaths {
    pub(crate) fn extend_owned(&mut self, other: Self) {
        self.skill_paths.extend(other.skill_paths);
        self.prompt_paths.extend(other.prompt_paths);
        self.theme_paths.extend(other.theme_paths);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.skill_paths.is_empty() && self.prompt_paths.is_empty() && self.theme_paths.is_empty()
    }

    pub(crate) fn clear(&mut self) {
        self.skill_paths.clear();
        self.prompt_paths.clear();
        self.theme_paths.clear();
    }
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

impl PromptTemplateInfo {
    pub fn slash_command_name(&self) -> &str {
        &self.name
    }
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
pub struct SkillDefinition {
    pub info: SkillInfo,
    pub content: String,
}

impl SkillInfo {
    pub fn slash_command_name(&self) -> String {
        format!("skill:{}", self.name)
    }
}

impl SkillCatalog {
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }
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
    pub registered_commands: Vec<RegisteredCommand>,
    pub registered_tools: Vec<RegisteredTool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptTemplateDefinition {
    pub info: PromptTemplateInfo,
    pub content: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionResourceBootstrap {
    pub extensions: ExtensionsResult,
    pub skills: Vec<SkillDefinition>,
    pub prompts: Vec<PromptTemplateDefinition>,
}

impl SessionResourceBootstrap {
    pub fn skill_catalog(&self) -> SkillCatalog {
        SkillCatalog {
            skills: self.skills.iter().map(|skill| skill.info.clone()).collect(),
        }
    }

    pub fn prompt_infos(&self) -> Vec<PromptTemplateInfo> {
        self.prompts
            .iter()
            .map(|prompt| prompt.info.clone())
            .collect()
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtensionCoreBindings {
    pub commands: Vec<SlashCommandInfo>,
    pub active_tools: Vec<String>,
    pub all_tools: Vec<String>,
    pub model: Option<ModelDescriptor>,
    pub system_prompt: String,
}
