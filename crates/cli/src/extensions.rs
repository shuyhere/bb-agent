use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use bb_core::agent_session_extensions::{
    ExtensionsResult, LoadedExtension, PromptTemplateDefinition, PromptTemplateInfo,
    RegisteredCommand, RegisteredTool, SessionResourceBootstrap, SkillDefinition, SkillInfo,
    SourceInfo, ToolDefinition,
};
use bb_core::config;
use bb_core::error::{BbError, BbResult};
use bb_core::settings::Settings;
use bb_core::types::ContentBlock;
use bb_plugin_host::{
    PluginContext, PluginHost, RegisteredCommand as HostRegisteredCommand,
    RegisteredTool as HostRegisteredTool,
};
use bb_tools::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ExtensionBootstrap {
    pub paths: Vec<PathBuf>,
    pub package_sources: Vec<String>,
}

impl ExtensionBootstrap {
    pub(crate) fn from_cli_values(cwd: &Path, values: &[String]) -> Self {
        let mut bootstrap = Self::default();
        for value in values {
            if is_package_source(value) {
                bootstrap.package_sources.push(value.clone());
            } else {
                bootstrap.paths.push(resolve_input_path(cwd, value));
            }
        }
        bootstrap
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct InputHookOutcome {
    pub handled: bool,
    pub text: Option<String>,
    pub output: Option<String>,
}

#[derive(Clone, Default)]
pub(crate) struct ExtensionCommandRegistry {
    host: Option<Arc<Mutex<PluginHost>>>,
    commands: BTreeSet<String>,
    context: PluginContext,
}

impl ExtensionCommandRegistry {
    pub(crate) fn is_registered(&self, text: &str) -> bool {
        parse_command_invocation(text)
            .map(|(name, _)| self.commands.contains(name))
            .unwrap_or(false)
    }

    pub(crate) async fn execute_text(&self, text: &str) -> Result<Option<String>> {
        let Some((name, args)) = parse_command_invocation(text) else {
            return Ok(None);
        };
        if !self.commands.contains(name) {
            return Ok(None);
        }

        let Some(host) = &self.host else {
            bail!("extension command runtime is not available");
        };
        let mut host = host.lock().await;
        let result = host
            .execute_command_with_context(name, args.unwrap_or_default(), &self.context)
            .await?;
        Ok(render_command_result(&result))
    }

    pub(crate) async fn send_event(&self, event: &bb_hooks::Event) -> Option<bb_hooks::HookResult> {
        let host = self.host.as_ref()?;
        let mut host = host.lock().await;
        host.send_event_with_context(event, &self.context).await
    }

    pub(crate) async fn apply_input_hooks(
        &self,
        text: &str,
        source: &str,
    ) -> Result<InputHookOutcome> {
        let Some(result) = self
            .send_event(&bb_hooks::Event::Input(bb_hooks::events::InputEvent {
                text: text.to_string(),
                source: source.to_string(),
            }))
            .await
        else {
            return Ok(InputHookOutcome {
                handled: false,
                text: Some(text.to_string()),
                output: None,
            });
        };

        let action = result.action.as_deref().unwrap_or("continue");
        let transformed_text = result.text.clone().or_else(|| Some(text.to_string()));
        let output = if action == "handled" {
            result
                .text
                .clone()
                .or_else(|| result.message.as_ref().and_then(render_command_result))
        } else {
            None
        };

        Ok(InputHookOutcome {
            handled: action == "handled",
            text: if action == "handled" {
                None
            } else {
                transformed_text
            },
            output,
        })
    }
}

pub(crate) struct RuntimeExtensionSupport {
    pub session_resources: SessionResourceBootstrap,
    pub tools: Vec<Box<dyn Tool>>,
    pub commands: ExtensionCommandRegistry,
}

impl Default for RuntimeExtensionSupport {
    fn default() -> Self {
        Self {
            session_resources: SessionResourceBootstrap::default(),
            tools: Vec::new(),
            commands: ExtensionCommandRegistry::default(),
        }
    }
}

pub(crate) async fn load_runtime_extension_support(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
) -> Result<RuntimeExtensionSupport> {
    load_runtime_extension_support_with_ui(cwd, settings, bootstrap, false).await
}

pub(crate) async fn load_runtime_extension_support_with_ui(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
    has_ui: bool,
) -> Result<RuntimeExtensionSupport> {
    let package_dirs = resolve_package_directories(cwd, settings, bootstrap)?;
    let mut discovered = DiscoveredResources::default();

    for root in default_extension_roots(cwd) {
        collect_extension_files_from_entry(
            &root,
            &mut discovered.extension_files,
            &mut discovered.extension_seen,
        );
    }
    for raw_path in &settings.extensions {
        collect_extension_files_from_entry(
            &resolve_input_path(cwd, raw_path),
            &mut discovered.extension_files,
            &mut discovered.extension_seen,
        );
    }
    for path in &bootstrap.paths {
        collect_extension_files_from_entry(
            path,
            &mut discovered.extension_files,
            &mut discovered.extension_seen,
        );
    }

    for root in default_skill_roots(cwd) {
        collect_skills_from_entry(
            &root,
            &mut discovered.skills,
            &mut discovered.skill_seen,
            cwd,
            None,
        );
    }
    for raw_path in &settings.skills {
        collect_skills_from_entry(
            &resolve_input_path(cwd, raw_path),
            &mut discovered.skills,
            &mut discovered.skill_seen,
            cwd,
            None,
        );
    }

    for root in default_prompt_roots(cwd) {
        collect_prompts_from_entry(
            &root,
            &mut discovered.prompts,
            &mut discovered.prompt_seen,
            cwd,
            None,
        );
    }
    for raw_path in &settings.prompts {
        collect_prompts_from_entry(
            &resolve_input_path(cwd, raw_path),
            &mut discovered.prompts,
            &mut discovered.prompt_seen,
            cwd,
            None,
        );
    }

    for package_dir in &package_dirs {
        let package_resources = discover_package_resources(package_dir, cwd)?;
        for entry in package_resources.extensions {
            collect_extension_files_from_entry(
                &entry,
                &mut discovered.extension_files,
                &mut discovered.extension_seen,
            );
        }
        for entry in package_resources.skills {
            collect_skills_from_entry(
                &entry,
                &mut discovered.skills,
                &mut discovered.skill_seen,
                cwd,
                Some(package_dir),
            );
        }
        for entry in package_resources.prompts {
            collect_prompts_from_entry(
                &entry,
                &mut discovered.prompts,
                &mut discovered.prompt_seen,
                cwd,
                Some(package_dir),
            );
        }
    }

    let mut session_resources = SessionResourceBootstrap {
        skills: if settings.enable_skill_commands {
            discovered.skills
        } else {
            Vec::new()
        },
        prompts: discovered.prompts,
        ..SessionResourceBootstrap::default()
    };

    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    let mut commands = ExtensionCommandRegistry::default();

    if !discovered.extension_files.is_empty() {
        let host = PluginHost::load_plugins(&discovered.extension_files).await?;
        let tool_registrations = host.registered_tools().to_vec();
        let command_registrations = host.registered_commands().to_vec();
        let shared_host = Arc::new(Mutex::new(host));

        tools = tool_registrations
            .iter()
            .cloned()
            .map(|registration| {
                Box::new(PluginTool::new(shared_host.clone(), registration)) as Box<dyn Tool>
            })
            .collect();

        commands = ExtensionCommandRegistry {
            host: Some(shared_host),
            commands: command_registrations
                .iter()
                .map(|command| command.name.clone())
                .collect(),
            context: PluginContext {
                cwd: Some(cwd.display().to_string()),
                has_ui,
            },
        };

        session_resources.extensions = ExtensionsResult {
            extensions: discovered
                .extension_files
                .iter()
                .map(|path| LoadedExtension {
                    path: path.display().to_string(),
                })
                .collect(),
            registered_commands: command_registrations
                .iter()
                .map(map_plugin_command_registration)
                .collect(),
            registered_tools: tool_registrations
                .iter()
                .map(map_plugin_tool_registration)
                .collect(),
            ..ExtensionsResult::default()
        };
    }

    Ok(RuntimeExtensionSupport {
        session_resources,
        tools,
        commands,
    })
}

pub(crate) fn install_package(source: &str, scope: SettingsScope, cwd: &Path) -> Result<()> {
    match classify_package_source(source) {
        PackageSource::LocalPath(path) => {
            let resolved = resolve_input_path(cwd, path);
            if !resolved.exists() {
                bail!("package path does not exist: {}", resolved.display());
            }
        }
        PackageSource::Npm(spec) => install_npm_package(spec)?,
        PackageSource::Git(spec) => install_git_package(spec)?,
    }

    let mut settings = load_settings_for_scope(scope, cwd);
    append_unique_package(&mut settings.packages, source.to_string(), cwd)?;
    save_settings_for_scope(scope, cwd, &settings)
}

pub(crate) fn remove_package(source: &str, scope: SettingsScope, cwd: &Path) -> Result<bool> {
    let mut settings = load_settings_for_scope(scope, cwd);
    let target_identity = package_identity(source, cwd)?;
    let before = settings.packages.len();
    settings.packages.retain(|entry| {
        package_identity(entry, cwd).ok().as_deref() != Some(target_identity.as_str())
    });
    let removed = before != settings.packages.len();
    if removed {
        save_settings_for_scope(scope, cwd, &settings)?;
    }
    Ok(removed)
}

pub(crate) fn list_packages(scope: Option<SettingsScope>, cwd: &Path) -> Vec<String> {
    match scope {
        Some(scope) => load_settings_for_scope(scope, cwd).packages,
        None => merge_package_lists(
            load_settings_for_scope(SettingsScope::Global, cwd).packages,
            load_settings_for_scope(SettingsScope::Project, cwd).packages,
            cwd,
        ),
    }
}

pub(crate) fn update_packages(scope: Option<SettingsScope>, cwd: &Path) -> Result<Vec<String>> {
    let packages = list_packages(scope, cwd);
    let mut updated = Vec::new();
    for package in &packages {
        if package_is_pinned(package) {
            continue;
        }
        match classify_package_source(package) {
            PackageSource::LocalPath(_) => {}
            PackageSource::Npm(spec) => install_npm_package(spec)?,
            PackageSource::Git(spec) => install_git_package(spec)?,
        }
        updated.push(package.clone());
    }
    Ok(updated)
}

#[derive(Default)]
struct DiscoveredResources {
    extension_files: Vec<PathBuf>,
    extension_seen: BTreeSet<String>,
    skills: Vec<SkillDefinition>,
    skill_seen: BTreeSet<String>,
    prompts: Vec<PromptTemplateDefinition>,
    prompt_seen: BTreeSet<String>,
}

#[derive(Default)]
struct PackageResources {
    extensions: Vec<PathBuf>,
    skills: Vec<PathBuf>,
    prompts: Vec<PathBuf>,
}

enum PackageSource<'a> {
    LocalPath(&'a str),
    Npm(&'a str),
    Git(&'a str),
}

fn default_extension_roots(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        config::global_dir().join("extensions"),
        config::project_dir(cwd).join("extensions"),
        cwd.join(".pi").join("extensions"),
    ];
    if let Some(home) = home_path() {
        roots.push(home.join(".pi").join("agent").join("extensions"));
    }
    roots
}

fn default_prompt_roots(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        config::global_dir().join("prompts"),
        config::project_dir(cwd).join("prompts"),
        cwd.join(".pi").join("prompts"),
    ];
    if let Some(home) = home_path() {
        roots.push(home.join(".pi").join("agent").join("prompts"));
    }
    roots
}

fn default_skill_roots(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        config::global_dir().join("skills"),
        config::project_dir(cwd).join("skills"),
        cwd.join(".pi").join("skills"),
    ];
    if let Some(home) = home_path() {
        roots.push(home.join(".pi").join("agent").join("skills"));
        roots.push(home.join(".agents").join("skills"));
    }
    for ancestor in cwd.ancestors() {
        roots.push(ancestor.join(".agents").join("skills"));
    }
    roots
}

fn collect_extension_files_from_entry(
    entry: &Path,
    files: &mut Vec<PathBuf>,
    seen: &mut BTreeSet<String>,
) {
    if !entry.exists() {
        return;
    }

    if entry.is_file() {
        if is_extension_file(entry) {
            push_unique_path(files, seen, entry.to_path_buf());
        }
        return;
    }

    let Ok(entries) = fs::read_dir(entry) else {
        return;
    };
    for child in entries.flatten().map(|value| value.path()) {
        if child.is_file() {
            if is_extension_file(&child) {
                push_unique_path(files, seen, child);
            }
        } else if child.is_dir() {
            if let Some(index) = resolve_extension_index(&child) {
                push_unique_path(files, seen, index);
            }
        }
    }
}

fn collect_skills_from_entry(
    entry: &Path,
    definitions: &mut Vec<SkillDefinition>,
    seen: &mut BTreeSet<String>,
    cwd: &Path,
    package_root: Option<&Path>,
) {
    if !entry.exists() {
        return;
    }

    if entry.is_file() {
        if entry.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
            || entry.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            push_skill_definition(entry, definitions, seen, cwd, package_root);
        }
        return;
    }

    let Ok(entries) = fs::read_dir(entry) else {
        return;
    };
    for child in entries.flatten().map(|value| value.path()) {
        if child.is_dir() {
            let skill_file = child.join("SKILL.md");
            if skill_file.is_file() {
                push_skill_definition(&skill_file, definitions, seen, cwd, package_root);
            }
            collect_skills_from_entry(&child, definitions, seen, cwd, package_root);
        } else if child.is_file() && child.extension().and_then(|ext| ext.to_str()) == Some("md") {
            push_skill_definition(&child, definitions, seen, cwd, package_root);
        }
    }
}

fn collect_prompts_from_entry(
    entry: &Path,
    definitions: &mut Vec<PromptTemplateDefinition>,
    seen: &mut BTreeSet<String>,
    cwd: &Path,
    package_root: Option<&Path>,
) {
    if !entry.exists() {
        return;
    }

    if entry.is_file() {
        if entry.extension().and_then(|ext| ext.to_str()) == Some("md") {
            push_prompt_definition(entry, definitions, seen, cwd, package_root);
        }
        return;
    }

    let Ok(entries) = fs::read_dir(entry) else {
        return;
    };
    for child in entries.flatten().map(|value| value.path()) {
        if child.is_file() && child.extension().and_then(|ext| ext.to_str()) == Some("md") {
            push_prompt_definition(&child, definitions, seen, cwd, package_root);
        }
    }
}

fn push_skill_definition(
    path: &Path,
    definitions: &mut Vec<SkillDefinition>,
    seen: &mut BTreeSet<String>,
    cwd: &Path,
    package_root: Option<&Path>,
) {
    let normalized = normalize_path(path.to_path_buf());
    let key = normalized.display().to_string();
    if !seen.insert(key.clone()) {
        return;
    }

    let Ok(content) = fs::read_to_string(&normalized) else {
        return;
    };
    let metadata = parse_frontmatter(&content);
    let fallback_name = normalized
        .parent()
        .and_then(|parent| parent.file_name())
        .or_else(|| normalized.file_stem())
        .and_then(|value| value.to_str())
        .unwrap_or("skill")
        .to_string();

    definitions.push(SkillDefinition {
        info: SkillInfo {
            name: metadata
                .get("name")
                .cloned()
                .filter(|value| !value.is_empty())
                .unwrap_or(fallback_name),
            description: metadata
                .get("description")
                .cloned()
                .unwrap_or_else(|| first_meaningful_line(&content).unwrap_or_default()),
            source_info: SourceInfo {
                path: key,
                source: resource_source_label(&normalized, cwd, package_root),
            },
        },
        content,
    });
}

fn push_prompt_definition(
    path: &Path,
    definitions: &mut Vec<PromptTemplateDefinition>,
    seen: &mut BTreeSet<String>,
    cwd: &Path,
    package_root: Option<&Path>,
) {
    let normalized = normalize_path(path.to_path_buf());
    let key = normalized.display().to_string();
    if !seen.insert(key.clone()) {
        return;
    }

    let Ok(content) = fs::read_to_string(&normalized) else {
        return;
    };

    definitions.push(PromptTemplateDefinition {
        info: PromptTemplateInfo {
            name: normalized
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("prompt")
                .to_string(),
            description: first_meaningful_line(&content).unwrap_or_default(),
            source_info: SourceInfo {
                path: key,
                source: resource_source_label(&normalized, cwd, package_root),
            },
        },
        content,
    });
}

fn resource_source_label(path: &Path, cwd: &Path, package_root: Option<&Path>) -> String {
    if let Some(root) = package_root {
        let name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("package");
        return format!("package:{name}");
    }
    if path.starts_with(config::global_dir()) {
        return "settings:global".to_string();
    }
    if let Some(home) = home_path() {
        if path.starts_with(home.join(".pi").join("agent")) {
            return "settings:global".to_string();
        }
    }
    if path.starts_with(config::project_dir(cwd))
        || path.starts_with(cwd.join(".pi"))
        || path.starts_with(cwd)
    {
        return "settings:project".to_string();
    }
    "settings:external".to_string()
}

fn parse_frontmatter(content: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return values;
    }

    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    values
}

fn first_meaningful_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "---" && !line.starts_with('#'))
        .map(ToOwned::to_owned)
}

fn resolve_input_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        normalize_path(path.to_path_buf())
    } else {
        normalize_path(base_dir.join(path))
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

fn push_unique_path(paths: &mut Vec<PathBuf>, seen: &mut BTreeSet<String>, path: PathBuf) {
    let normalized = normalize_path(path);
    let key = normalized.display().to_string();
    if seen.insert(key) {
        paths.push(normalized);
    }
}

fn resolve_extension_index(dir: &Path) -> Option<PathBuf> {
    ["index.ts", "index.js", "index.mjs", "index.cjs"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

fn is_extension_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ts" | "js" | "mjs" | "cjs")
    )
}

fn home_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn parse_command_invocation(text: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = text.trim();
    let remainder = trimmed.strip_prefix('/')?;
    split_command_name_and_args(remainder)
}

fn split_command_name_and_args(input: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed.find(char::is_whitespace) {
        Some(index) => {
            let name = trimmed[..index].trim();
            if name.is_empty() {
                return None;
            }
            let args = trimmed[index..].trim();
            Some((name, (!args.is_empty()).then_some(args)))
        }
        None => Some((trimmed, None)),
    }
}

fn render_command_result(value: &Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("message").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    Some(serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
}

fn is_package_source(value: &str) -> bool {
    value.starts_with("npm:")
        || value.starts_with("git:")
        || value.starts_with("https://")
        || value.starts_with("http://")
        || value.starts_with("ssh://")
        || value.starts_with("git://")
}

fn classify_package_source(source: &str) -> PackageSource<'_> {
    if let Some(spec) = source.strip_prefix("npm:") {
        PackageSource::Npm(spec)
    } else if source.starts_with("git:")
        || source.starts_with("https://")
        || source.starts_with("http://")
        || source.starts_with("ssh://")
        || source.starts_with("git://")
    {
        PackageSource::Git(source)
    } else {
        PackageSource::LocalPath(source)
    }
}

fn load_settings_for_scope(scope: SettingsScope, cwd: &Path) -> Settings {
    match scope {
        SettingsScope::Global => Settings::load_global(),
        SettingsScope::Project => Settings::load_project(cwd),
    }
}

fn save_settings_for_scope(scope: SettingsScope, cwd: &Path, settings: &Settings) -> Result<()> {
    match scope {
        SettingsScope::Global => settings.save_global().map_err(Into::into),
        SettingsScope::Project => settings.save_project(cwd).map_err(Into::into),
    }
}

fn append_unique_package(values: &mut Vec<String>, value: String, cwd: &Path) -> Result<()> {
    let identity = package_identity(&value, cwd)?;
    if let Some(existing_index) = values.iter().position(|existing| {
        package_identity(existing, cwd).ok().as_deref() == Some(identity.as_str())
    }) {
        values[existing_index] = value;
    } else {
        values.push(value);
    }
    Ok(())
}

fn merge_package_lists(global: Vec<String>, project: Vec<String>, cwd: &Path) -> Vec<String> {
    let mut merged = global;
    for package in project {
        let _ = append_unique_package(&mut merged, package, cwd);
    }
    merged
}

fn package_identity(source: &str, cwd: &Path) -> Result<String> {
    match classify_package_source(source) {
        PackageSource::LocalPath(path) => {
            Ok(format!("local:{}", resolve_input_path(cwd, path).display()))
        }
        PackageSource::Npm(spec) => Ok(format!("npm:{}", npm_package_name(spec)?)),
        PackageSource::Git(spec) => Ok(format!("git:{}", git_repo_url(spec))),
    }
}

fn package_is_pinned(source: &str) -> bool {
    match classify_package_source(source) {
        PackageSource::LocalPath(_) => false,
        PackageSource::Npm(spec) => npm_package_name(spec)
            .map(|name| name != spec)
            .unwrap_or(false),
        PackageSource::Git(spec) => git_ref(spec).is_some(),
    }
}

fn resolve_package_directories(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
) -> Result<Vec<PathBuf>> {
    let mut package_dirs = Vec::new();
    let mut seen = BTreeSet::new();

    for source in settings
        .packages
        .iter()
        .chain(bootstrap.package_sources.iter())
    {
        let path = resolve_package_directory(cwd, source)?;
        push_unique_path(&mut package_dirs, &mut seen, path);
    }

    Ok(package_dirs)
}

fn resolve_package_directory(cwd: &Path, source: &str) -> Result<PathBuf> {
    match classify_package_source(source) {
        PackageSource::LocalPath(path) => Ok(resolve_input_path(cwd, path)),
        PackageSource::Npm(spec) => resolve_npm_package_dir(spec),
        PackageSource::Git(spec) => Ok(git_package_install_dir(spec)),
    }
}

fn discover_package_resources(package_dir: &Path, cwd: &Path) -> Result<PackageResources> {
    if !package_dir.exists() {
        return Ok(PackageResources::default());
    }

    let manifest = package_dir.join("package.json");
    if manifest.is_file() {
        let package_json: Value = serde_json::from_str(
            &fs::read_to_string(&manifest)
                .with_context(|| format!("read {}", manifest.display()))?,
        )
        .with_context(|| format!("parse {}", manifest.display()))?;

        if let Some(pi) = package_json.get("pi").and_then(Value::as_object) {
            return Ok(PackageResources {
                extensions: manifest_entries(package_dir, pi.get("extensions")),
                skills: manifest_entries(package_dir, pi.get("skills")),
                prompts: manifest_entries(package_dir, pi.get("prompts")),
            });
        }
    }

    let mut resources = PackageResources::default();
    for (dir_name, target) in [
        ("extensions", &mut resources.extensions),
        ("skills", &mut resources.skills),
        ("prompts", &mut resources.prompts),
    ] {
        let path = package_dir.join(dir_name);
        if path.exists() {
            target.push(normalize_path(path));
        }
    }

    if resources.extensions.is_empty()
        && resources.skills.is_empty()
        && resources.prompts.is_empty()
        && package_dir.starts_with(cwd)
        && (package_dir.is_dir() || package_dir.is_file())
    {
        resources
            .extensions
            .push(normalize_path(package_dir.to_path_buf()));
    }

    Ok(resources)
}

fn manifest_entries(package_dir: &Path, value: Option<&Value>) -> Vec<PathBuf> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|entry| normalize_path(package_dir.join(entry)))
                .collect()
        })
        .unwrap_or_default()
}

fn package_install_root(kind: &str, spec: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(spec.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    config::global_dir().join(kind).join(hash)
}

fn install_npm_package(spec: &str) -> Result<()> {
    let install_root = package_install_root("npm", spec);
    fs::create_dir_all(&install_root)?;
    run_command(
        Command::new("npm")
            .arg("install")
            .arg(spec)
            .current_dir(&install_root),
        &format!("npm install {spec}"),
    )
}

fn resolve_npm_package_dir(spec: &str) -> Result<PathBuf> {
    let install_root = package_install_root("npm", spec);
    let package_name = npm_package_name(spec)?;
    Ok(install_root.join("node_modules").join(package_name))
}

fn npm_package_name(spec: &str) -> Result<String> {
    if spec.is_empty() {
        bail!("empty npm package spec");
    }
    if let Some(rest) = spec.strip_prefix('@') {
        let second_at = rest.rfind('@');
        let candidate = match second_at {
            Some(index) if rest[..index].contains('/') => &spec[..index + 1],
            _ => spec,
        };
        Ok(candidate.to_string())
    } else {
        Ok(spec
            .rsplit_once('@')
            .map(|(name, _)| name)
            .unwrap_or(spec)
            .to_string())
    }
}

fn install_git_package(spec: &str) -> Result<()> {
    let install_root = git_package_install_dir(spec);
    let repo = git_repo_url(spec);
    if install_root.exists() {
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&install_root)
                .arg("pull")
                .arg("--ff-only"),
            &format!("git pull {}", install_root.display()),
        )?;
    } else {
        if let Some(parent) = install_root.parent() {
            fs::create_dir_all(parent)?;
        }
        run_command(
            Command::new("git")
                .arg("clone")
                .arg(&repo)
                .arg(&install_root),
            &format!("git clone {repo}"),
        )?;
    }

    if let Some(reference) = git_ref(spec) {
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&install_root)
                .arg("checkout")
                .arg(reference),
            &format!("git checkout {reference}"),
        )?;
    }

    if install_root.join("package.json").is_file() {
        run_command(
            Command::new("npm")
                .arg("install")
                .current_dir(&install_root),
            &format!("npm install in {}", install_root.display()),
        )?;
    }

    Ok(())
}

fn git_package_install_dir(spec: &str) -> PathBuf {
    package_install_root("git", spec)
}

fn git_repo_url(spec: &str) -> &str {
    let stripped = spec.strip_prefix("git:").unwrap_or(spec);
    strip_git_ref(stripped).0
}

fn git_ref(spec: &str) -> Option<&str> {
    let stripped = spec.strip_prefix("git:").unwrap_or(spec);
    strip_git_ref(stripped).1
}

fn strip_git_ref(spec: &str) -> (&str, Option<&str>) {
    let last_at = spec.rfind('@');
    let Some(index) = last_at else {
        return (spec, None);
    };
    let slash_index = spec.rfind('/').unwrap_or(0);
    let colon_index = spec.rfind(':').unwrap_or(0);
    if index > slash_index.max(colon_index) {
        (&spec[..index], Some(&spec[index + 1..]))
    } else {
        (spec, None)
    }
}

fn run_command(command: &mut Command, description: &str) -> Result<()> {
    let status = command.status().with_context(|| description.to_string())?;
    if !status.success() {
        bail!("{description} failed with status {status}");
    }
    Ok(())
}

fn map_plugin_command_registration(command: &HostRegisteredCommand) -> RegisteredCommand {
    RegisteredCommand {
        invocation_name: command.name.clone(),
        description: command.description.clone(),
        source_info: SourceInfo {
            path: format!("<command:{}>", command.name),
            source: "extension:plugin-host".to_string(),
        },
    }
}

fn map_plugin_tool_registration(tool: &HostRegisteredTool) -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: tool.name.clone(),
            prompt_snippet: None,
            prompt_guidelines: Vec::new(),
        },
        source_info: SourceInfo {
            path: format!("<tool:{}>", tool.name),
            source: "extension:plugin-host".to_string(),
        },
    }
}

struct PluginTool {
    host: Arc<Mutex<PluginHost>>,
    registration: HostRegisteredTool,
}

impl PluginTool {
    fn new(host: Arc<Mutex<PluginHost>>, registration: HostRegisteredTool) -> Self {
        Self { host, registration }
    }
}

#[async_trait]
impl Tool for PluginTool {
    fn name(&self) -> &str {
        &self.registration.name
    }

    fn description(&self) -> &str {
        &self.registration.description
    }

    fn parameters_schema(&self) -> Value {
        self.registration.parameters.clone()
    }

    async fn execute(
        &self,
        params: Value,
        _ctx: &ToolContext,
        _cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let mut host = self.host.lock().await;
        let result = host
            .execute_tool(self.name(), self.name(), params)
            .await
            .map_err(|err| BbError::Plugin(err.to_string()))?;
        map_tool_result(result)
    }
}

fn map_tool_result(value: Value) -> BbResult<ToolResult> {
    let mut content = Vec::new();

    if let Some(blocks) = value.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        content.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
                "image" => {
                    let data = block.get("data").and_then(Value::as_str);
                    let mime_type = block
                        .get("mime_type")
                        .or_else(|| block.get("mimeType"))
                        .and_then(Value::as_str);
                    if let (Some(data), Some(mime_type)) = (data, mime_type) {
                        content.push(ContentBlock::Image {
                            data: data.to_string(),
                            mime_type: mime_type.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
    }

    if content.is_empty() {
        if let Some(text) = value.get("text").and_then(Value::as_str) {
            content.push(ContentBlock::Text {
                text: text.to_string(),
            });
        } else {
            content.push(ContentBlock::Text {
                text: serde_json::to_string_pretty(&value).map_err(BbError::Json)?,
            });
        }
    }

    Ok(ToolResult {
        content,
        details: value.get("details").cloned(),
        is_error: value
            .get("isError")
            .or_else(|| value.get("is_error"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        artifact_path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn node_available() -> bool {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_ok()
    }

    #[test]
    fn parses_frontmatter_name_and_description() {
        let metadata =
            parse_frontmatter("---\nname: demo-skill\ndescription: Helpful skill\n---\n# Demo");
        assert_eq!(metadata.get("name"), Some(&"demo-skill".to_string()));
        assert_eq!(
            metadata.get("description"),
            Some(&"Helpful skill".to_string())
        );
    }

    #[test]
    fn parses_command_invocation_and_args() {
        assert_eq!(
            parse_command_invocation("/hello world"),
            Some(("hello", Some("world")))
        );
        assert_eq!(parse_command_invocation("/hello"), Some(("hello", None)));
        assert_eq!(parse_command_invocation("hello"), None);
    }

    #[test]
    fn classifies_package_sources() {
        assert!(matches!(
            classify_package_source("npm:demo"),
            PackageSource::Npm(_)
        ));
        assert!(matches!(
            classify_package_source("git:https://x"),
            PackageSource::Git(_)
        ));
        assert!(matches!(
            classify_package_source("./local"),
            PackageSource::LocalPath(_)
        ));
    }

    #[test]
    fn discovers_package_resources_from_manifest() {
        let cwd = tempdir().unwrap();
        let package_dir = cwd.path().join("demo-package");
        fs::create_dir_all(package_dir.join("pkg-extensions")).unwrap();
        fs::create_dir_all(package_dir.join("pkg-skills")).unwrap();
        fs::create_dir_all(package_dir.join("pkg-prompts")).unwrap();
        fs::write(
            package_dir.join("package.json"),
            r#"{
                "name": "demo-package",
                "pi": {
                    "extensions": ["./pkg-extensions"],
                    "skills": ["./pkg-skills"],
                    "prompts": ["./pkg-prompts"]
                }
            }"#,
        )
        .unwrap();

        let resources = discover_package_resources(&package_dir, cwd.path()).unwrap();
        assert_eq!(
            resources.extensions,
            vec![normalize_path(package_dir.join("pkg-extensions"))]
        );
        assert_eq!(
            resources.skills,
            vec![normalize_path(package_dir.join("pkg-skills"))]
        );
        assert_eq!(
            resources.prompts,
            vec![normalize_path(package_dir.join("pkg-prompts"))]
        );
    }

    #[tokio::test]
    async fn loads_package_skills_and_prompts_from_settings() {
        let cwd = tempdir().unwrap();
        let package_dir = cwd.path().join("skills-package");
        fs::create_dir_all(package_dir.join("skills/review")).unwrap();
        fs::create_dir_all(package_dir.join("prompts")).unwrap();
        fs::write(
            package_dir.join("package.json"),
            r#"{
                "name": "skills-package",
                "pi": {
                    "skills": ["./skills"],
                    "prompts": ["./prompts"]
                }
            }"#,
        )
        .unwrap();
        fs::write(
            package_dir.join("skills/review/SKILL.md"),
            "---\nname: package-review\ndescription: package review skill\n---\nReview carefully.",
        )
        .unwrap();
        fs::write(
            package_dir.join("prompts/summarize.md"),
            "Summarize the package state.",
        )
        .unwrap();

        let settings = Settings {
            packages: vec![package_dir.display().to_string()],
            ..Settings::default()
        };
        let support =
            load_runtime_extension_support(cwd.path(), &settings, &ExtensionBootstrap::default())
                .await
                .unwrap();

        assert!(
            support
                .session_resources
                .skills
                .iter()
                .any(|skill| skill.info.name == "package-review")
        );
        assert!(
            support
                .session_resources
                .prompts
                .iter()
                .any(|prompt| prompt.info.name == "summarize")
        );
    }

    #[test]
    fn project_scoped_package_settings_round_trip() {
        let cwd = tempdir().unwrap();
        let package_dir = cwd.path().join("local-package");
        fs::create_dir_all(&package_dir).unwrap();

        install_package(
            package_dir.to_str().unwrap(),
            SettingsScope::Project,
            cwd.path(),
        )
        .unwrap();

        let listed = list_packages(Some(SettingsScope::Project), cwd.path());
        assert_eq!(listed, vec![package_dir.display().to_string()]);

        let updated = update_packages(Some(SettingsScope::Project), cwd.path()).unwrap();
        assert_eq!(updated, vec![package_dir.display().to_string()]);

        assert!(
            remove_package(
                package_dir.to_str().unwrap(),
                SettingsScope::Project,
                cwd.path(),
            )
            .unwrap()
        );
        assert!(list_packages(Some(SettingsScope::Project), cwd.path()).is_empty());
    }

    #[test]
    fn package_identity_controls_remove_and_listing() {
        let cwd = tempdir().unwrap();
        let merged = merge_package_lists(
            vec!["npm:@demo/pkg@1.0.0".to_string()],
            vec!["npm:@demo/pkg@2.0.0".to_string()],
            cwd.path(),
        );
        assert_eq!(merged, vec!["npm:@demo/pkg@2.0.0".to_string()]);

        let settings = Settings {
            packages: vec!["npm:@demo/pkg@2.0.0".to_string()],
            ..Settings::default()
        };
        settings.save_project(cwd.path()).unwrap();

        assert!(remove_package("npm:@demo/pkg", SettingsScope::Project, cwd.path()).unwrap());
        assert!(list_packages(Some(SettingsScope::Project), cwd.path()).is_empty());
    }

    #[test]
    fn update_skips_pinned_package_sources() {
        let cwd = tempdir().unwrap();
        let package_dir = cwd.path().join("local-package");
        fs::create_dir_all(&package_dir).unwrap();

        let settings = Settings {
            packages: vec![
                package_dir.display().to_string(),
                "npm:@demo/pinned@1.2.3".to_string(),
                "git:https://example.com/repo@v1".to_string(),
            ],
            ..Settings::default()
        };
        settings.save_project(cwd.path()).unwrap();

        let updated = update_packages(Some(SettingsScope::Project), cwd.path()).unwrap();
        assert!(updated.contains(&package_dir.display().to_string()));
        assert!(!updated.contains(&"npm:@demo/pinned@1.2.3".to_string()));
        assert!(!updated.contains(&"git:https://example.com/repo@v1".to_string()));
    }

    #[tokio::test]
    async fn package_loaded_extension_command_executes_with_context() {
        if !node_available() {
            eprintln!("Skipping test: node not available");
            return;
        }

        let cwd = tempdir().unwrap();
        let package_dir = cwd.path().join("command-package");
        fs::create_dir_all(package_dir.join("extensions")).unwrap();
        fs::write(
            package_dir.join("package.json"),
            r#"{
                "name": "command-package",
                "pi": {
                    "extensions": ["./extensions"]
                }
            }"#,
        )
        .unwrap();
        fs::write(
            package_dir.join("extensions/hello.js"),
            r#"
                module.exports = function(bb) {
                    bb.registerCommand('pkghello', {
                        description: 'package hello',
                        handler: async (args, ctx) => ({
                            message: `pkg:${args}|ui:${ctx.hasUI}|cwd:${ctx.cwd}`,
                        }),
                    });
                };
            "#,
        )
        .unwrap();

        let settings = Settings {
            packages: vec![package_dir.display().to_string()],
            ..Settings::default()
        };
        let support = load_runtime_extension_support_with_ui(
            cwd.path(),
            &settings,
            &ExtensionBootstrap::default(),
            true,
        )
        .await
        .unwrap();

        assert!(support.commands.is_registered("/pkghello world"));
        let output = support
            .commands
            .execute_text("/pkghello world")
            .await
            .unwrap();
        let output = output.unwrap();
        assert!(output.contains("pkg:world"));
        assert!(output.contains("ui:true"));
        assert!(output.contains(cwd.path().to_str().unwrap()));
    }

    #[tokio::test]
    async fn reload_reloads_extension_command_output() {
        if !node_available() {
            eprintln!("Skipping test: node not available");
            return;
        }

        let cwd = tempdir().unwrap();
        let extension_path = cwd.path().join("reload.js");
        fs::write(
            &extension_path,
            r#"
                module.exports = function(bb) {
                    bb.registerCommand('hello', {
                        description: 'hello',
                        handler: async () => ({ message: 'v1' }),
                    });
                };
            "#,
        )
        .unwrap();

        let bootstrap = ExtensionBootstrap {
            paths: vec![extension_path.clone()],
            package_sources: Vec::new(),
        };
        let settings = Settings::default();
        let support_v1 = load_runtime_extension_support(cwd.path(), &settings, &bootstrap)
            .await
            .unwrap();
        assert_eq!(
            support_v1.commands.execute_text("/hello").await.unwrap(),
            Some("v1".to_string())
        );

        fs::write(
            &extension_path,
            r#"
                module.exports = function(bb) {
                    bb.registerCommand('hello', {
                        description: 'hello',
                        handler: async () => ({ message: 'v2' }),
                    });
                };
            "#,
        )
        .unwrap();

        let support_v2 = load_runtime_extension_support(cwd.path(), &settings, &bootstrap)
            .await
            .unwrap();
        assert_eq!(
            support_v2.commands.execute_text("/hello").await.unwrap(),
            Some("v2".to_string())
        );
    }
}
