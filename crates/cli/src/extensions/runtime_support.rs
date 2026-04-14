use super::discovery::resolve_input_path;
use super::packages::is_package_source;
use super::plugin_runtime::build_plugin_runtime;
use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct ExtensionBootstrap {
    pub paths: Vec<PathBuf>,
    pub package_sources: Vec<String>,
}

impl ExtensionBootstrap {
    /// Split CLI `--extension` values into package sources vs. local/runtime
    /// paths before extension loading begins.
    ///
    /// Examples:
    /// - `npm:demo-skill` stays in `package_sources` for package resolution
    /// - `./local-ext` is resolved into `paths`
    pub(crate) fn from_cli_values(cwd: &Path, values: &[String]) -> Self {
        let mut bootstrap = Self {
            paths: Vec::with_capacity(values.len()),
            package_sources: Vec::with_capacity(values.len()),
        };
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

#[derive(Default)]
pub(crate) struct RuntimeExtensionSupport {
    pub session_resources: SessionResourceBootstrap,
    pub tools: Vec<Box<dyn Tool>>,
    pub commands: ExtensionCommandRegistry,
}

/// Render the prompt section that advertises discovered skills and prompt
/// templates to the agent.
///
/// This is shared by `bb run`, TUI startup, and session bootstrap so the agent
/// sees the same skill/prompt inventory regardless of entry point.
pub(crate) fn build_skill_system_prompt_section(resources: &SessionResourceBootstrap) -> String {
    let mut sections = Vec::new();

    if !resources.skills.is_empty() {
        let mut skill_lines = Vec::new();
        skill_lines.push("<available_skills>".to_string());
        for skill in &resources.skills {
            skill_lines.push("  <skill>".to_string());
            skill_lines.push(format!("    <name>{}</name>", skill.info.name));
            skill_lines.push(format!(
                "    <description>{}</description>",
                skill.info.description
            ));
            skill_lines.push(format!(
                "    <location>{}</location>",
                skill.info.source_info.path
            ));
            skill_lines.push("  </skill>".to_string());
        }
        skill_lines.push("</available_skills>".to_string());
        sections.push(skill_lines.join("\n"));
    }

    if !resources.prompts.is_empty() {
        let prompt_list: Vec<String> = resources
            .prompts
            .iter()
            .map(|prompt| format!("- /{}: {}", prompt.info.name, prompt.info.description))
            .collect();
        sections.push(format!(
            "Available prompt templates (invoke with /name):\n{}",
            prompt_list.join("\n")
        ));
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", sections.join("\n\n"))
    }
}

/// Load extension runtime support without attaching an interactive UI handler.
pub(crate) async fn load_runtime_extension_support(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
) -> Result<RuntimeExtensionSupport> {
    load_runtime_extension_support_with_ui(cwd, settings, bootstrap, false).await
}

/// Load extension runtime support and, when requested, wire in the interactive
/// UI bridge used by the TUI auth/notification flows.
pub(crate) async fn load_runtime_extension_support_with_ui(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
    has_ui: bool,
) -> Result<RuntimeExtensionSupport> {
    let package_dirs = resolve_package_directories(cwd, settings, bootstrap)?;
    let discovered = discover_runtime_resources(cwd, settings, bootstrap, &package_dirs)?;

    let mut session_resources = SessionResourceBootstrap {
        skills: if settings.enable_skill_commands {
            discovered.skills
        } else {
            Vec::new()
        },
        prompts: discovered.prompts,
        ..SessionResourceBootstrap::default()
    };

    let (tools, commands, extensions) =
        build_plugin_runtime(cwd, has_ui, &discovered.extension_files).await?;
    session_resources.extensions = extensions;

    Ok(RuntimeExtensionSupport {
        session_resources,
        tools,
        commands,
    })
}
