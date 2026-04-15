use anyhow::{Context, Result, anyhow};
use bb_core::agent_session_runtime::RuntimeModelRef;
use bb_core::settings::Settings;
use bb_tui::tui::{TuiCommand, TuiNoteLevel};

use crate::agents_md::load_agents_md;
use crate::extensions::{
    ExtensionCommandOutcome, RuntimeExtensionSupport, SettingsScope, auto_install_missing_packages,
    build_skill_system_prompt_section, install_package, load_runtime_extension_support_with_ui,
};
use crate::slash::{
    InstallSlashAction, SkillAdminAction, dispatch_local_slash_command, install_help_text,
    parse_install_command, skill_help_text,
};
use crate::tool_registry::ToolRegistry;
use crate::tui::build_dynamic_slash_items;
use crate::tui::controller::QueuedPrompt;
use crate::turn_runner;
use crate::update_check::{self, UpdateCheckOutcome};

use super::{ResourceWatchState, TuiController};

impl TuiController {
    pub(crate) async fn handle_local_submission(&mut self, text: &str) -> Result<bool> {
        let text = text.trim();

        if text == "/reload" {
            self.reload_runtime_resources().await?;
            self.show_startup_resources();
            return Ok(true);
        }

        if text == "/update" {
            self.check_for_updates_now().await?;
            return Ok(true);
        }

        if text == "/compact" || text.starts_with("/compact ") {
            if self.streaming || self.manual_compaction_in_progress {
                self.queued_prompts
                    .push_back(QueuedPrompt::Visible(text.to_string()));
                self.send_command(TuiCommand::SetStatusLine(
                    if self.manual_compaction_in_progress {
                        "Queued /compact to run after the current compaction".to_string()
                    } else {
                        "Queued /compact to run after the current turn".to_string()
                    },
                ));
                self.publish_status();
                return Ok(true);
            }
            let instructions = text
                .strip_prefix("/compact")
                .map(str::trim)
                .filter(|s| !s.is_empty());
            self.handle_compact_command(instructions).await?;
            return Ok(true);
        }

        if let Some(install) = parse_install_command(text) {
            match install {
                InstallSlashAction::Help => {
                    self.send_command(TuiCommand::PushNote {
                        level: TuiNoteLevel::Status,
                        text: install_help_text(),
                    });
                }
                InstallSlashAction::Install(install) => {
                    self.install_and_reload_package(install.local, install.source)
                        .await?;
                }
            }
            return Ok(true);
        }

        if let Some(action) = crate::slash::parse_skill_command(text) {
            self.handle_skill_admin_command(action).await?;
            return Ok(true);
        }

        // Extension-registered slash commands take precedence over falling
        // through to the LLM. Matches the one-shot `bb run` dispatch path.
        if self.session_setup.extension_commands.is_registered(text) {
            self.execute_extension_command_text(text).await?;
            return Ok(true);
        }

        match dispatch_local_slash_command(self, text) {
            Ok(handled) => Ok(handled),
            Err(err) => {
                tracing::error!("local command error: {err}");
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Error,
                    text: format!("Command error: {err}"),
                });
                Ok(true)
            }
        }
    }

    async fn check_for_updates_now(&mut self) -> Result<()> {
        if self.streaming {
            self.send_command(TuiCommand::SetStatusLine(
                "Cannot check for updates while a turn is running".to_string(),
            ));
            return Ok(());
        }

        self.send_command(TuiCommand::SetStatusLine(
            "Checking for updates...".to_string(),
        ));
        match update_check::check_for_updates(true, &self.session_setup.tool_ctx.cwd).await? {
            UpdateCheckOutcome::Disabled => {
                self.send_command(TuiCommand::SetStatusLine(
                    "Update check is not configured yet".to_string(),
                ));
            }
            UpdateCheckOutcome::UpToDate => {
                self.send_command(TuiCommand::SetStatusLine(format!(
                    "Already up to date (v{})",
                    env!("CARGO_PKG_VERSION")
                )));
            }
            UpdateCheckOutcome::UpdateAvailable(notice) => {
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Highlight,
                    text: update_check::build_update_available_note(&notice),
                });
                self.send_command(TuiCommand::SetStatusLine(format!(
                    "Update available: {}",
                    notice.latest_version
                )));
            }
        }
        Ok(())
    }

    async fn install_and_reload_package(&mut self, local: bool, source: String) -> Result<()> {
        if self.streaming {
            self.send_command(TuiCommand::SetStatusLine(
                "Cannot install packages while a turn is running".to_string(),
            ));
            return Ok(());
        }

        let cwd = self.session_setup.tool_ctx.cwd.clone();
        let scope = if local {
            SettingsScope::Project
        } else {
            SettingsScope::Global
        };
        self.send_command(TuiCommand::SetStatusLine(format!("Installing {source}...")));

        let source_for_install = source.clone();
        tokio::task::spawn_blocking(move || install_package(&source_for_install, scope, &cwd))
            .await
            .map_err(|err| anyhow!("install task failed: {err}"))??;

        self.reload_runtime_resources().await?;
        self.send_command(TuiCommand::SetStatusLine(format!(
            "Installed {source} and reloaded resources"
        )));
        Ok(())
    }

    pub(crate) async fn execute_extension_command_text(&mut self, text: &str) -> Result<()> {
        let outcome = match self
            .session_setup
            .extension_commands
            .execute_text_structured(text)
            .await
        {
            Ok(outcome) => outcome,
            Err(err) => {
                tracing::error!("extension command error: {err}");
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Error,
                    text: format!("Extension command error: {err}"),
                });
                return Ok(());
            }
        };

        match outcome {
            ExtensionCommandOutcome::Nothing => {
                self.pending_extension_prompt = None;
                self.send_command(TuiCommand::CloseAuthDialog);
                self.send_command(TuiCommand::SetLocalActionActive(false));
            }
            ExtensionCommandOutcome::Text(text) => {
                self.pending_extension_prompt = None;
                self.send_command(TuiCommand::CloseAuthDialog);
                self.send_command(TuiCommand::SetLocalActionActive(false));
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Status,
                    text,
                });
            }
            ExtensionCommandOutcome::Dispatch { note, prompt } => {
                // Close any open local UI (wizard dialog, menu) since the
                // extension has finished collecting input and we're about
                // to hand control back to the agent.
                self.pending_extension_prompt = None;
                self.send_command(TuiCommand::CloseAuthDialog);
                self.send_command(TuiCommand::CloseSelectMenu);
                self.send_command(TuiCommand::SetLocalActionActive(false));
                if let Some(note) = note {
                    self.send_command(TuiCommand::PushNote {
                        level: TuiNoteLevel::Highlight,
                        text: note,
                    });
                }
                // Queue the dispatch prompt as a hidden internal turn. The
                // user sees only the short kickoff note above; the long
                // orchestration prompt stays out of the transcript while the
                // resulting tool activity still streams normally.
                self.queued_prompts.push_back(QueuedPrompt::Hidden(prompt));
                self.publish_status();
            }
            ExtensionCommandOutcome::ActivateAgent { agent_id, note } => {
                self.pending_extension_prompt = None;
                self.send_command(TuiCommand::CloseAuthDialog);
                self.send_command(TuiCommand::CloseSelectMenu);
                self.send_command(TuiCommand::SetLocalActionActive(false));
                self.activate_saved_shape_agent(&agent_id, note.as_deref())?;
            }
            ExtensionCommandOutcome::Menu {
                command,
                title,
                items,
            } => {
                self.send_command(TuiCommand::CloseAuthDialog);
                self.open_extension_menu(command, title, items);
            }
            ExtensionCommandOutcome::Prompt(prompt) => {
                self.open_extension_prompt(prompt);
            }
        }
        Ok(())
    }

    fn activate_saved_shape_agent(&mut self, agent_id: &str, note: Option<&str>) -> Result<()> {
        let home = std::env::var("HOME").context("HOME is not set")?;
        let agent_dir = std::path::Path::new(&home)
            .join(".bb-agent")
            .join("agents")
            .join(agent_id);
        let system_prompt_path = agent_dir.join("SYSTEM_PROMPT.md");
        let agent_json_path = agent_dir.join("agent.json");

        let system_prompt = std::fs::read_to_string(&system_prompt_path).with_context(|| {
            format!(
                "failed to read shaped agent prompt: {}",
                system_prompt_path.display()
            )
        })?;

        #[derive(serde::Deserialize)]
        struct ShapedAgentIdentity {
            role: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct ShapedAgentResource {
            knowledge_pages: Option<u64>,
        }
        #[derive(serde::Deserialize)]
        struct ShapedAgentSkill {
            name: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct ShapedAgentMeta {
            name: Option<String>,
            identity: Option<ShapedAgentIdentity>,
            resource: Option<ShapedAgentResource>,
            skills: Option<Vec<ShapedAgentSkill>>,
        }

        let meta: ShapedAgentMeta = serde_json::from_str(
            &std::fs::read_to_string(&agent_json_path).with_context(|| {
                format!(
                    "failed to read shaped agent metadata: {}",
                    agent_json_path.display()
                )
            })?,
        )
        .with_context(|| {
            format!(
                "failed to parse shaped agent metadata: {}",
                agent_json_path.display()
            )
        })?;

        let agents_md = load_agents_md(&self.session_setup.tool_ctx.cwd);
        let base_prompt = bb_core::agent::build_system_prompt(&system_prompt, agents_md.as_deref());
        self.session_setup.base_system_prompt = base_prompt;
        self.session_setup.system_prompt = format!(
            "{}{}",
            self.session_setup.base_system_prompt,
            build_skill_system_prompt_section(&self.runtime_host.bootstrap().resource_bootstrap)
        );

        let agent_name = meta.name.unwrap_or_else(|| agent_id.to_string());
        let role = meta
            .identity
            .and_then(|identity| identity.role)
            .unwrap_or_else(|| "Shaped Agent".to_string());
        let knowledge_pages = meta
            .resource
            .and_then(|resource| resource.knowledge_pages)
            .unwrap_or(0);
        let skills = meta.skills.unwrap_or_default();
        let skill_count = skills.iter().filter(|skill| skill.name.is_some()).count();

        let summary = format!(
            "✅ Activated: {agent_name}\nRole: {role}\nKnowledge: {knowledge_pages} pages | Skills: {skill_count}"
        );
        self.send_command(TuiCommand::PushNote {
            level: TuiNoteLevel::Highlight,
            text: note.map(str::to_string).unwrap_or(summary),
        });
        self.send_command(TuiCommand::SetStatusLine(format!(
            "Active agent: {agent_name}"
        )));
        Ok(())
    }

    fn open_extension_menu(
        &mut self,
        command: String,
        title: String,
        items: Vec<crate::extensions::ExtensionMenuItem>,
    ) {
        self.pending_extension_prompt = None;
        self.send_command(TuiCommand::SetLocalActionActive(true));
        let menu_id = format!(
            "ext:{command}:{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let select_items: Vec<bb_tui::select_list::SelectItem> = items
            .into_iter()
            .map(|item| bb_tui::select_list::SelectItem {
                label: item.label,
                detail: item.detail,
                value: item.value,
            })
            .collect();
        if select_items.is_empty() {
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Warning,
                text: format!("/{command} returned an empty menu"),
            });
            return;
        }
        self.pending_extension_menus
            .insert(menu_id.clone(), command);
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id,
            title,
            items: select_items,
            selected_value: None,
        });
    }

    fn open_extension_prompt(&mut self, prompt: crate::extensions::ExtensionPromptSpec) {
        self.pending_extension_prompt = Some(prompt.clone());
        self.send_command(TuiCommand::SetLocalActionActive(true));
        self.send_command(TuiCommand::CloseSelectMenu);
        self.send_command(TuiCommand::OpenAuthDialog(bb_tui::tui::TuiAuthDialog {
            title: prompt.title,
            status: None,
            steps: Vec::new(),
            url: None,
            lines: prompt.lines,
            input_label: prompt.input_label,
            input_placeholder: prompt.input_placeholder,
        }));
        self.send_command(TuiCommand::SetInput(String::new()));
    }

    pub(crate) async fn handle_skill_admin_command(
        &mut self,
        action: SkillAdminAction,
    ) -> Result<()> {
        match action {
            SkillAdminAction::Help => {
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Status,
                    text: skill_help_text(),
                });
            }
            SkillAdminAction::List => {
                let loaded: Vec<String> = self
                    .runtime_host
                    .bootstrap()
                    .resource_bootstrap
                    .skills
                    .iter()
                    .map(|skill| skill.info.name.clone())
                    .collect();

                let settings = Settings::load_merged(&self.session_setup.tool_ctx.cwd);
                let disabled: Vec<String> = settings
                    .disabled_skills
                    .iter()
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .collect();

                let mut lines = Vec::new();
                lines.push("Loaded skills:".to_string());
                if loaded.is_empty() {
                    lines.push("  (none)".to_string());
                } else {
                    for name in &loaded {
                        lines.push(format!("  • {name}"));
                    }
                }
                lines.push(String::new());
                lines.push("Disabled skills (source kept on disk):".to_string());
                if disabled.is_empty() {
                    lines.push("  (none)".to_string());
                } else {
                    for name in &disabled {
                        lines.push(format!("  • {name}"));
                    }
                }
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Status,
                    text: lines.join("\n"),
                });
            }
            SkillAdminAction::Disable(name) => {
                self.apply_skill_disable(&name, true).await?;
            }
            SkillAdminAction::Enable(name) => {
                self.apply_skill_disable(&name, false).await?;
            }
        }
        Ok(())
    }

    async fn apply_skill_disable(&mut self, name: &str, disable: bool) -> Result<()> {
        if self.streaming {
            self.send_command(TuiCommand::SetStatusLine(
                "Cannot modify skills while a turn is running".to_string(),
            ));
            return Ok(());
        }

        let trimmed = name.trim();
        if trimmed.is_empty() {
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Warning,
                text: "Missing skill name. See /skill for usage.".to_string(),
            });
            return Ok(());
        }

        // Mutate global settings. The disable list is a global-scoped
        // preference; users can still override per-project with manual
        // JSON edits if they need to.
        let mut settings = Settings::load_global();
        let normalized = trimmed.to_string();
        let normalized_lower = normalized.to_ascii_lowercase();
        let already = settings
            .disabled_skills
            .iter()
            .any(|entry| entry.trim().eq_ignore_ascii_case(&normalized));

        if disable {
            if already {
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Status,
                    text: format!("Skill '{normalized}' is already disabled."),
                });
                return Ok(());
            }
            // Only warn if there is no matching currently-loaded skill; the
            // user may still want to pre-disable a skill they plan to install.
            let known = self
                .runtime_host
                .bootstrap()
                .resource_bootstrap
                .skills
                .iter()
                .any(|skill| skill.info.name.eq_ignore_ascii_case(&normalized));
            if !known {
                self.send_command(TuiCommand::PushNote {
                    level: TuiNoteLevel::Warning,
                    text: format!(
                        "Note: no loaded skill named '{normalized}'. Recording the disable anyway.",
                    ),
                });
            }
            settings.disabled_skills.push(normalized.clone());
        } else if !already {
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Status,
                text: format!("Skill '{normalized}' is not disabled."),
            });
            return Ok(());
        } else {
            settings
                .disabled_skills
                .retain(|entry| !entry.trim().eq_ignore_ascii_case(&normalized_lower));
        }

        if let Err(err) = settings.save_global() {
            self.send_command(TuiCommand::PushNote {
                level: TuiNoteLevel::Error,
                text: format!("Failed to persist disabled_skills: {err}"),
            });
            return Ok(());
        }

        // Reload so the change takes effect in this session immediately.
        self.reload_runtime_resources().await?;
        self.show_startup_resources();
        self.send_command(TuiCommand::SetStatusLine(if disable {
            format!("Disabled skill: {normalized}")
        } else {
            format!("Enabled skill: {normalized}")
        }));
        Ok(())
    }

    pub(crate) async fn maybe_auto_reload_resources(&mut self) -> Result<()> {
        let next_watch = ResourceWatchState::capture(&self.session_setup.tool_ctx.cwd);
        if next_watch == self.resource_watch {
            return Ok(());
        }
        if self.suppress_next_resource_watch_reload {
            self.resource_watch = next_watch;
            self.suppress_next_resource_watch_reload = false;
            return Ok(());
        }
        if self.streaming {
            return Ok(());
        }

        self.reload_runtime_resources().await?;
        self.send_command(TuiCommand::SetStatusLine(
            "Detected package/settings change and reloaded resources".to_string(),
        ));
        Ok(())
    }

    pub(crate) async fn reload_runtime_resources(&mut self) -> Result<()> {
        if self.streaming {
            self.send_command(TuiCommand::SetStatusLine(
                "Cannot reload resources while a turn is running".to_string(),
            ));
            return Ok(());
        }

        let cwd = self.session_setup.tool_ctx.cwd.clone();
        self.send_command(TuiCommand::SetStatusLine(
            "Reloading extensions, skills, and prompts...".to_string(),
        ));

        let settings = Settings::load_merged(&cwd);
        auto_install_missing_packages(&cwd, &settings);
        let _ = self
            .session_setup
            .extension_commands
            .send_event(&bb_hooks::Event::SessionShutdown)
            .await;

        let RuntimeExtensionSupport {
            session_resources,
            tools,
            mut commands,
        } = load_runtime_extension_support_with_ui(
            &cwd,
            &settings,
            &self.session_setup.extension_bootstrap,
            true,
        )
        .await?;

        let sibling_conn = if let Some(conn) = self.session_setup.sibling_conn.clone() {
            conn
        } else {
            let conn = turn_runner::open_sibling_conn(&self.session_setup.conn)?;
            self.session_setup.sibling_conn = Some(conn.clone());
            conn
        };
        commands.bind_session_context(sibling_conn, self.session_setup.session_id.clone(), None);
        let _ = commands.send_event(&bb_hooks::Event::SessionStart).await;

        self.session_setup.tool_registry = ToolRegistry::from_builtin_and_extensions(
            tools,
            self.session_setup.tool_selection.clone(),
        );
        self.session_setup.extension_commands = commands;
        self.session_setup.system_prompt = format!(
            "{}{}",
            self.session_setup.base_system_prompt,
            build_skill_system_prompt_section(&session_resources)
        );

        self.runtime_host.reload_resources(session_resources);
        self.runtime_host
            .runtime_mut()
            .set_model(Some(RuntimeModelRef {
                provider: self.session_setup.model.provider.clone(),
                id: self.session_setup.model.id.clone(),
                context_window: self.session_setup.model.context_window as usize,
            }));
        self.resource_watch = ResourceWatchState::capture(&self.session_setup.tool_ctx.cwd);
        self.send_command(TuiCommand::SetExtraSlashItems(build_dynamic_slash_items(
            &self.runtime_host,
        )));
        self.publish_footer();
        Ok(())
    }
}
