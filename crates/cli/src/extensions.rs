use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
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
use bb_core::settings::{PackageEntry, Settings};
use bb_core::types::{ContentBlock, SessionEntry};
use bb_plugin_host::{
    DefaultUiHandler, PluginContext, PluginHost, RegisteredCommand as HostRegisteredCommand,
    RegisteredTool as HostRegisteredTool, SharedUiHandler, UiHandler, UiRequest, UiResponse,
    default_ui_response,
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

#[derive(Clone)]
struct SessionSnapshotSource {
    conn: Arc<Mutex<rusqlite::Connection>>,
    session_id: String,
    session_file: Option<String>,
}

/// Print-mode UI handler: logs notifications to tracing, returns defaults for dialogs.
#[derive(Clone, Debug, Default)]
pub(crate) struct PrintUiHandler;

impl UiHandler for PrintUiHandler {
    fn handle_request(
        &self,
        request: UiRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = UiResponse> + Send + '_>> {
        Box::pin(async move {
            match request.method.as_str() {
                "notify" => {
                    let msg = request
                        .params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let kind = request
                        .params
                        .get("notifyType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info");
                    match kind {
                        "error" => tracing::error!("[extension] {msg}"),
                        "warning" => tracing::warn!("[extension] {msg}"),
                        _ => tracing::info!("[extension] {msg}"),
                    }
                }
                "setStatus" | "setWidget" | "setTitle" | "set_editor_text" => {
                    // No-op in print mode
                }
                _ => {}
            }
            default_ui_response(&request)
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Interactive-mode UI handler: stores notifications/statuses and can be
/// queried by the interactive controller. Dialogs return defaults for now
/// but the plumbing supports real TUI dialogs in the future.
#[derive(Clone, Debug)]
pub(crate) struct InteractiveUiHandler {
    notifications: Arc<Mutex<Vec<UiNotification>>>,
    statuses: Arc<Mutex<BTreeMap<String, Option<String>>>>,
}

#[derive(Clone, Debug)]
pub(crate) struct UiNotification {
    pub message: String,
    pub kind: String,
}

impl Default for InteractiveUiHandler {
    fn default() -> Self {
        Self {
            notifications: Arc::new(Mutex::new(Vec::new())),
            statuses: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl InteractiveUiHandler {
    /// Drain all pending notifications.
    pub(crate) async fn drain_notifications(&self) -> Vec<UiNotification> {
        let mut notifications = self.notifications.lock().await;
        std::mem::take(&mut *notifications)
    }

    /// Get all current status entries.
    pub(crate) async fn get_statuses(&self) -> BTreeMap<String, Option<String>> {
        self.statuses.lock().await.clone()
    }
}

impl UiHandler for InteractiveUiHandler {
    fn handle_request(
        &self,
        request: UiRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = UiResponse> + Send + '_>> {
        let notifications = self.notifications.clone();
        let statuses = self.statuses.clone();
        Box::pin(async move {
            match request.method.as_str() {
                "notify" => {
                    let msg = request
                        .params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let kind = request
                        .params
                        .get("notifyType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string();
                    notifications
                        .lock()
                        .await
                        .push(UiNotification { message: msg, kind });
                }
                "setStatus" => {
                    let key = request
                        .params
                        .get("statusKey")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let text = request
                        .params
                        .get("statusText")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string);
                    statuses.lock().await.insert(key, text);
                }
                "setWidget" | "setTitle" | "set_editor_text" => {
                    // Store for future consumption by interactive controller
                    tracing::debug!("Interactive UI: {} (stored for controller)", request.method);
                }
                _ => {}
            }
            default_ui_response(&request)
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Clone, Default)]
pub(crate) struct ExtensionCommandRegistry {
    host: Option<Arc<Mutex<PluginHost>>>,
    commands: BTreeSet<String>,
    context: PluginContext,
    session: Option<SessionSnapshotSource>,
    ui_handler: Option<SharedUiHandler>,
}

impl fmt::Debug for ExtensionCommandRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionCommandRegistry")
            .field("commands", &self.commands)
            .field("context", &self.context)
            .field("has_host", &self.host.is_some())
            .field("has_session", &self.session.is_some())
            .field("has_ui_handler", &self.ui_handler.is_some())
            .finish_non_exhaustive()
    }
}

impl ExtensionCommandRegistry {
    pub(crate) fn bind_session_context(
        &mut self,
        conn: Arc<Mutex<rusqlite::Connection>>,
        session_id: impl Into<String>,
        session_file: Option<String>,
    ) {
        self.session = Some(SessionSnapshotSource {
            conn,
            session_id: session_id.into(),
            session_file,
        });
    }

    async fn build_context(&self) -> PluginContext {
        let mut context = self.context.clone();
        let Some(session) = &self.session else {
            return context;
        };

        let conn = session.conn.lock().await;
        let entries =
            bb_session::store::get_entries(&conn, &session.session_id).unwrap_or_default();
        let branch = bb_session::tree::active_path(&conn, &session.session_id).unwrap_or_default();
        let session_row = bb_session::store::get_session(&conn, &session.session_id)
            .ok()
            .flatten();
        drop(conn);

        context.session_entries = entries
            .into_iter()
            .filter_map(|row| bb_session::store::parse_entry(&row).ok())
            .filter_map(|entry| serde_json::to_value(entry).ok())
            .collect();
        context.session_branch = branch
            .into_iter()
            .filter_map(|row| bb_session::store::parse_entry(&row).ok())
            .filter_map(|entry| serde_json::to_value(entry).ok())
            .collect();
        context.leaf_id = session_row.as_ref().and_then(|row| row.leaf_id.clone());
        context.session_name = session_row.as_ref().and_then(|row| row.name.clone());
        context.session_file = session.session_file.clone();
        context.session_id = Some(session.session_id.clone());
        context.labels = build_labels_map(&context.session_entries);
        context
    }

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
        let context = self.build_context().await;
        let mut host = host.lock().await;
        let result = host
            .execute_command_with_context(name, args.unwrap_or_default(), &context)
            .await?;
        Ok(render_command_result(&result))
    }

    pub(crate) async fn send_event(&self, event: &bb_hooks::Event) -> Option<bb_hooks::HookResult> {
        let host = self.host.as_ref()?;
        let context = self.build_context().await;
        let mut host = host.lock().await;
        host.send_event_with_context(event, &context).await
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

    for resolved in &package_dirs {
        let package_resources = discover_package_resources(&resolved.dir, cwd)?;
        let ext_filter = resolved.entry.extensions_filter();
        let skill_filter = resolved.entry.skills_filter();
        let prompt_filter = resolved.entry.prompts_filter();

        // Collect extensions (then filter by path)
        if !matches!(ext_filter, Some(f) if f.is_empty()) {
            let before = discovered.extension_files.len();
            for entry in &package_resources.extensions {
                collect_extension_files_from_entry(
                    entry,
                    &mut discovered.extension_files,
                    &mut discovered.extension_seen,
                );
            }
            // Apply filter to newly collected items
            if ext_filter.is_some() {
                apply_path_filter(
                    &mut discovered.extension_files,
                    &mut discovered.extension_seen,
                    before,
                    &resolved.dir,
                    ext_filter,
                );
            }
        }

        // Collect skills (then filter by path)
        if !matches!(skill_filter, Some(f) if f.is_empty()) {
            let before = discovered.skills.len();
            for entry in &package_resources.skills {
                collect_skills_from_entry(
                    entry,
                    &mut discovered.skills,
                    &mut discovered.skill_seen,
                    cwd,
                    Some(&resolved.dir),
                );
            }
            // Apply filter to newly collected items
            if let Some(patterns) = skill_filter {
                let retained: Vec<_> = discovered.skills[before..]
                    .iter()
                    .filter(|s| {
                        filter_matches(
                            Path::new(&s.info.source_info.path),
                            &resolved.dir,
                            Some(patterns),
                        )
                    })
                    .cloned()
                    .collect();
                discovered.skills.truncate(before);
                discovered.skills.extend(retained);
            }
        }

        // Collect prompts (then filter by path)
        if !matches!(prompt_filter, Some(f) if f.is_empty()) {
            let before = discovered.prompts.len();
            for entry in &package_resources.prompts {
                collect_prompts_from_entry(
                    entry,
                    &mut discovered.prompts,
                    &mut discovered.prompt_seen,
                    cwd,
                    Some(&resolved.dir),
                );
            }
            // Apply filter to newly collected items
            if let Some(patterns) = prompt_filter {
                let retained: Vec<_> = discovered.prompts[before..]
                    .iter()
                    .filter(|p| {
                        filter_matches(
                            Path::new(&p.info.source_info.path),
                            &resolved.dir,
                            Some(patterns),
                        )
                    })
                    .cloned()
                    .collect();
                discovered.prompts.truncate(before);
                discovered.prompts.extend(retained);
            }
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

    let ui_handler: SharedUiHandler = if has_ui {
        Arc::new(InteractiveUiHandler::default())
    } else {
        Arc::new(PrintUiHandler)
    };

    if !discovered.extension_files.is_empty() {
        let mut host = PluginHost::load_plugins(&discovered.extension_files).await?;
        host.set_ui_handler(ui_handler.clone());
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
                ..PluginContext::default()
            },
            session: None,
            ui_handler: Some(ui_handler.clone()),
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
        PackageSource::Npm(spec) => install_npm_package(spec, scope, cwd)?,
        PackageSource::Git(spec) => install_git_package(spec, scope, cwd)?,
    }

    let mut settings = load_settings_for_scope(scope, cwd);
    append_unique_package(
        &mut settings.packages,
        PackageEntry::Simple(source.to_string()),
        cwd,
    )?;
    save_settings_for_scope(scope, cwd, &settings)
}

pub(crate) fn remove_package(source: &str, scope: SettingsScope, cwd: &Path) -> Result<bool> {
    let mut settings = load_settings_for_scope(scope, cwd);
    let target_identity = package_identity(source, cwd)?;
    let before = settings.packages.len();
    settings.packages.retain(|entry| {
        package_identity(entry.source(), cwd).ok().as_deref() != Some(target_identity.as_str())
    });
    let removed = before != settings.packages.len();
    if removed {
        save_settings_for_scope(scope, cwd, &settings)?;
    }
    Ok(removed)
}

pub(crate) fn list_packages(scope: Option<SettingsScope>, cwd: &Path) -> Vec<String> {
    let entries = match scope {
        Some(scope) => load_settings_for_scope(scope, cwd).packages,
        None => {
            use bb_core::settings::Settings;
            let global = load_settings_for_scope(SettingsScope::Global, cwd).packages;
            let project = load_settings_for_scope(SettingsScope::Project, cwd).packages;
            bb_core::settings::Settings::merge(
                &Settings {
                    packages: global,
                    ..Settings::default()
                },
                &Settings {
                    packages: project,
                    ..Settings::default()
                },
            )
            .packages
        }
    };
    entries.iter().map(|e| e.source().to_string()).collect()
}

pub(crate) fn update_packages(scope: Option<SettingsScope>, cwd: &Path) -> Result<Vec<String>> {
    let packages = list_packages(scope, cwd);
    let mut updated = Vec::new();
    for package in &packages {
        if package_is_pinned(package) {
            continue;
        }
        let effective_scope = scope.unwrap_or(SettingsScope::Global);
        match classify_package_source(package) {
            PackageSource::LocalPath(_) => {}
            PackageSource::Npm(spec) => install_npm_package(spec, effective_scope, cwd)?,
            PackageSource::Git(spec) => install_git_package(spec, effective_scope, cwd)?,
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

fn append_unique_package(
    values: &mut Vec<PackageEntry>,
    value: PackageEntry,
    cwd: &Path,
) -> Result<()> {
    let identity = package_identity(value.source(), cwd)?;
    if let Some(existing_index) = values.iter().position(|existing| {
        package_identity(existing.source(), cwd).ok().as_deref() == Some(identity.as_str())
    }) {
        values[existing_index] = value;
    } else {
        values.push(value);
    }
    Ok(())
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

/// A resolved package directory together with its optional filter.
struct ResolvedPackage {
    dir: PathBuf,
    entry: PackageEntry,
}

fn resolve_package_directories(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
) -> Result<Vec<ResolvedPackage>> {
    let mut resolved = Vec::new();
    let mut seen = BTreeSet::new();

    for entry in &settings.packages {
        let path = resolve_package_directory(cwd, entry.source())?;
        let key = normalize_path(path.clone()).display().to_string();
        if seen.insert(key) {
            resolved.push(ResolvedPackage {
                dir: path,
                entry: entry.clone(),
            });
        }
    }

    for source in &bootstrap.package_sources {
        let path = resolve_package_directory(cwd, source)?;
        let key = normalize_path(path.clone()).display().to_string();
        if seen.insert(key) {
            resolved.push(ResolvedPackage {
                dir: path,
                entry: PackageEntry::Simple(source.clone()),
            });
        }
    }

    Ok(resolved)
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

/// Check if a path passes a package filter.
///
/// - `None` filter (omitted) = matches everything
/// - `Some([])` = matches nothing (caller should skip before calling)
/// - Patterns:
///   - `!pattern` excludes paths ending with pattern
///   - `+path` force-includes exact relative path
///   - `-path` force-excludes exact relative path
///   - otherwise includes if path contains the pattern
fn filter_matches(path: &Path, package_root: &Path, filter: Option<&[String]>) -> bool {
    let Some(patterns) = filter else {
        return true; // No filter = include all
    };
    if patterns.is_empty() {
        return false; // Empty filter = include none
    }

    let relative = path
        .strip_prefix(package_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.display().to_string());

    let mut included = false;
    let mut force_excluded = false;
    let mut force_included = false;

    for pattern in patterns {
        if let Some(exact) = pattern.strip_prefix('+') {
            if relative == exact || path.ends_with(exact) {
                force_included = true;
            }
        } else if let Some(exact) = pattern.strip_prefix('-') {
            if relative == exact || path.ends_with(exact) {
                force_excluded = true;
            }
        } else if let Some(exclude) = pattern.strip_prefix('!') {
            if relative.ends_with(exclude) || relative.contains(exclude) {
                force_excluded = true;
            }
        } else {
            // Include pattern: path contains or matches
            if relative.contains(pattern.as_str()) || path.ends_with(pattern.as_str()) {
                included = true;
            }
        }
    }

    if force_excluded && !force_included {
        return false;
    }
    if force_included {
        return true;
    }
    included
}

/// Apply a path filter to a range of collected PathBufs, removing non-matching ones.
fn apply_path_filter(
    paths: &mut Vec<PathBuf>,
    seen: &mut BTreeSet<String>,
    start: usize,
    package_root: &Path,
    filter: Option<&[String]>,
) {
    let retained: Vec<PathBuf> = paths[start..]
        .iter()
        .filter(|p| filter_matches(p, package_root, filter))
        .cloned()
        .collect();
    // Remove keys for filtered-out paths
    for removed in &paths[start..] {
        if !retained.iter().any(|r| r == removed) {
            seen.remove(&removed.display().to_string());
        }
    }
    paths.truncate(start);
    paths.extend(retained);
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

/// Scope-aware install root for npm/git packages.
///
/// - Global: `~/.bb-agent/<kind>/<hash>`
/// - Project: `<cwd>/.bb-agent/<kind>/<hash>`
fn package_install_root(kind: &str, spec: &str, scope: SettingsScope, cwd: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(spec.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    match scope {
        SettingsScope::Global => config::global_dir().join(kind).join(hash),
        SettingsScope::Project => config::project_dir(cwd).join(kind).join(hash),
    }
}

/// Resolve install root: check project-local first, then global.
fn resolve_install_root(kind: &str, spec: &str, cwd: &Path) -> PathBuf {
    let project = package_install_root(kind, spec, SettingsScope::Project, cwd);
    if project.exists() {
        return project;
    }
    package_install_root(kind, spec, SettingsScope::Global, cwd)
}

fn install_npm_package(spec: &str, scope: SettingsScope, cwd: &Path) -> Result<()> {
    let install_root = package_install_root("npm", spec, scope, cwd);
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
    // Check both project-local and global install roots
    // (resolve_install_root checks project first, falls back to global)
    let install_root_global = package_install_root("npm", spec, SettingsScope::Global, Path::new("."));
    let package_name = npm_package_name(spec)?;
    Ok(install_root_global.join("node_modules").join(package_name))
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

fn install_git_package(spec: &str, scope: SettingsScope, cwd: &Path) -> Result<()> {
    let install_root = package_install_root("git", spec, scope, cwd);
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
    // Resolve: check project-local first, then global
    resolve_install_root("git", spec, Path::new("."))
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

fn build_labels_map(entries: &[Value]) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    for entry in entries {
        if let Ok(SessionEntry::Label {
            target_id, label, ..
        }) = serde_json::from_value::<SessionEntry>(entry.clone())
        {
            match label {
                Some(label) => {
                    labels.insert(target_id.to_string(), label);
                }
                None => {
                    labels.remove(&target_id.to_string());
                }
            }
        }
    }
    labels
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
            packages: vec![PackageEntry::Simple(package_dir.display().to_string())],
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
        // Use Settings::merge to test package dedup
        let global = Settings {
            packages: vec![PackageEntry::Simple("npm:@demo/pkg@1.0.0".to_string())],
            ..Settings::default()
        };
        let project = Settings {
            packages: vec![PackageEntry::Simple("npm:@demo/pkg@2.0.0".to_string())],
            ..Settings::default()
        };
        let merged = Settings::merge(&global, &project);
        assert_eq!(merged.packages.len(), 1);
        assert_eq!(merged.packages[0].source(), "npm:@demo/pkg@2.0.0");

        let settings = Settings {
            packages: vec![PackageEntry::Simple("npm:@demo/pkg@2.0.0".to_string())],
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
                PackageEntry::Simple(package_dir.display().to_string()),
                PackageEntry::Simple("npm:@demo/pinned@1.2.3".to_string()),
                PackageEntry::Simple("git:https://example.com/repo@v1".to_string()),
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
                            message: [
                                `pkg:${args}`,
                                `ui:${ctx.hasUI}`,
                                `cwd:${ctx.cwd}`,
                                `entries:${ctx.sessionManager.getEntries().length}`,
                                `branch:${ctx.sessionManager.getBranch().length}`,
                                `leaf:${ctx.sessionManager.getLeafId()}`,
                                `label:${ctx.sessionManager.getLabel(ctx.sessionManager.getEntries()[0]?.id)}`,
                                `session:${ctx.sessionManager.getSessionId()}`,
                            ].join('|'),
                        }),
                    });
                };
            "#,
        )
        .unwrap();

        let conn = bb_session::store::open_db(&cwd.path().join("sessions.db")).unwrap();
        let session_id =
            bb_session::store::create_session(&conn, cwd.path().to_str().unwrap()).unwrap();
        let root = bb_core::types::SessionEntry::Message {
            base: bb_core::types::EntryBase {
                id: bb_core::types::EntryId::generate(),
                parent_id: None,
                timestamp: chrono::Utc::now(),
            },
            message: bb_core::types::AgentMessage::User(bb_core::types::UserMessage {
                content: vec![bb_core::types::ContentBlock::Text {
                    text: "hello".to_string(),
                }],
                timestamp: chrono::Utc::now().timestamp_millis(),
            }),
        };
        let root_id = root.base().id.to_string();
        bb_session::store::append_entry(&conn, &session_id, &root).unwrap();
        let label = bb_core::types::SessionEntry::Label {
            base: bb_core::types::EntryBase {
                id: bb_core::types::EntryId::generate(),
                parent_id: Some(bb_core::types::EntryId(root_id.clone())),
                timestamp: chrono::Utc::now(),
            },
            target_id: bb_core::types::EntryId(root_id.clone()),
            label: Some("root-label".to_string()),
        };
        bb_session::store::append_entry(&conn, &session_id, &label).unwrap();

        let settings = Settings {
            packages: vec![PackageEntry::Simple(package_dir.display().to_string())],
            ..Settings::default()
        };
        let mut support = load_runtime_extension_support_with_ui(
            cwd.path(),
            &settings,
            &ExtensionBootstrap::default(),
            true,
        )
        .await
        .unwrap();
        support.commands.bind_session_context(
            crate::turn_runner::open_sibling_conn(&conn).unwrap(),
            session_id.clone(),
            None,
        );

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
        assert!(output.contains("entries:2"));
        assert!(output.contains("branch:2"));
        assert!(output.contains(&format!("leaf:{}", label.base().id)));
        assert!(output.contains("label:root-label"));
        assert!(output.contains(&format!("session:{session_id}")));
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

    #[test]
    fn filter_matches_patterns() {
        let root = Path::new("/pkg");

        // None filter = include all
        assert!(filter_matches(Path::new("/pkg/ext/a.ts"), root, None));

        // Empty filter = include none
        assert!(!filter_matches(
            Path::new("/pkg/ext/a.ts"),
            root,
            Some(&[])
        ));

        // Positive match
        assert!(filter_matches(
            Path::new("/pkg/ext/a.ts"),
            root,
            Some(&["ext/a.ts".to_string()])
        ));

        // No match
        assert!(!filter_matches(
            Path::new("/pkg/ext/b.ts"),
            root,
            Some(&["ext/a.ts".to_string()])
        ));

        // Exclusion
        assert!(!filter_matches(
            Path::new("/pkg/ext/legacy.ts"),
            root,
            Some(&["ext".to_string(), "!legacy.ts".to_string()])
        ));

        // Force include overrides exclusion
        assert!(filter_matches(
            Path::new("/pkg/ext/legacy.ts"),
            root,
            Some(&["!legacy.ts".to_string(), "+ext/legacy.ts".to_string()])
        ));

        // Force exclude
        assert!(!filter_matches(
            Path::new("/pkg/ext/a.ts"),
            root,
            Some(&["ext".to_string(), "-ext/a.ts".to_string()])
        ));
    }

    #[tokio::test]
    async fn filtered_package_loads_only_matching_resources() {
        let cwd = tempdir().unwrap();
        let package_dir = cwd.path().join("filtered-pkg");
        fs::create_dir_all(package_dir.join("skills/review")).unwrap();
        fs::create_dir_all(package_dir.join("skills/debug")).unwrap();
        fs::create_dir_all(package_dir.join("prompts")).unwrap();
        fs::write(
            package_dir.join("package.json"),
            r#"{
                "name": "filtered-pkg",
                "pi": {
                    "skills": ["./skills"],
                    "prompts": ["./prompts"]
                }
            }"#,
        )
        .unwrap();
        fs::write(
            package_dir.join("skills/review/SKILL.md"),
            "---\nname: review\ndescription: review skill\n---\nReview.",
        )
        .unwrap();
        fs::write(
            package_dir.join("skills/debug/SKILL.md"),
            "---\nname: debug\ndescription: debug skill\n---\nDebug.",
        )
        .unwrap();
        fs::write(
            package_dir.join("prompts/summarize.md"),
            "Summarize.",
        )
        .unwrap();
        fs::write(
            package_dir.join("prompts/fixtest.md"),
            "Fix tests.",
        )
        .unwrap();

        // Load with filter: only review skill, no prompts
        let settings = Settings {
            packages: vec![PackageEntry::Filtered(bb_core::settings::PackageFilter {
                source: package_dir.display().to_string(),
                extensions: None,
                skills: Some(vec!["review".to_string()]),
                prompts: Some(vec![]),
            })],
            ..Settings::default()
        };
        let support =
            load_runtime_extension_support(cwd.path(), &settings, &ExtensionBootstrap::default())
                .await
                .unwrap();

        // Only review skill should be loaded
        let skill_names: Vec<&str> = support
            .session_resources
            .skills
            .iter()
            .map(|s| s.info.name.as_str())
            .collect();
        assert!(skill_names.contains(&"review"), "review should be loaded");
        assert!(!skill_names.contains(&"debug"), "debug should be filtered out");

        // No prompts should be loaded (empty filter)
        assert!(support.session_resources.prompts.is_empty(), "prompts should be empty");
    }

    #[tokio::test]
    async fn extension_ui_notify_and_confirm_plumbing() {
        if !node_available() {
            eprintln!("Skipping test: node not available");
            return;
        }

        let cwd = tempdir().unwrap();
        let ext_path = cwd.path().join("ui-ext.js");
        fs::write(
            &ext_path,
            r#"
                module.exports = function(bb) {
                    bb.registerCommand('ui-demo', {
                        description: 'demo UI methods',
                        handler: async (args, ctx) => {
                            ctx.ui.notify('extension says hi', 'info');
                            ctx.ui.setStatus('demo', 'active');
                            const ok = await ctx.ui.confirm('Title', 'Sure?');
                            const picked = await ctx.ui.select('Pick', ['a','b']);
                            return { message: `ok=${ok} picked=${picked}` };
                        },
                    });
                };
            "#,
        )
        .unwrap();

        let bootstrap = ExtensionBootstrap {
            paths: vec![ext_path],
            package_sources: Vec::new(),
        };
        let settings = Settings::default();
        // Load with has_ui=true to get an InteractiveUiHandler
        let support =
            load_runtime_extension_support_with_ui(cwd.path(), &settings, &bootstrap, true)
                .await
                .unwrap();

        // Get the interactive handler to verify stored notifications
        let handler = support
            .commands
            .ui_handler
            .as_ref()
            .expect("should have ui handler");
        // Downcast to InteractiveUiHandler
        let interactive_handler = handler
            .as_ref()
            .as_any()
            .downcast_ref::<InteractiveUiHandler>()
            .expect("should be InteractiveUiHandler");

        let output = support
            .commands
            .execute_text("/ui-demo")
            .await
            .unwrap()
            .unwrap();
        // Dialogs return defaults: confirm=false, select=cancelled(undefined)
        assert_eq!(output, "ok=false picked=undefined");

        // Verify notifications were captured
        let notifications = interactive_handler.drain_notifications().await;
        assert!(!notifications.is_empty());
        assert_eq!(notifications[0].message, "extension says hi");
        assert_eq!(notifications[0].kind, "info");

        // Verify status was captured
        let statuses = interactive_handler.get_statuses().await;
        assert_eq!(statuses.get("demo"), Some(&Some("active".to_string())));
    }
}
