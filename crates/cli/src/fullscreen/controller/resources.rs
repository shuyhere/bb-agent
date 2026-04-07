use anyhow::{Result, anyhow};
use bb_core::agent_session_runtime::RuntimeModelRef;
use bb_core::settings::Settings;
use bb_tools::builtin_tools;
use bb_tui::fullscreen::{FullscreenCommand, FullscreenNoteLevel};

use crate::extensions::{
    RuntimeExtensionSupport, SettingsScope, auto_install_missing_packages,
    build_skill_system_prompt_section, install_package, load_runtime_extension_support_with_ui,
};
use crate::fullscreen::build_dynamic_slash_items;
use crate::session_bootstrap::build_tool_defs;
use crate::slash::{
    InstallSlashAction, dispatch_local_slash_command, install_help_text, parse_install_command,
};
use crate::turn_runner;
use crate::update_check::{self, UpdateCheckOutcome};

use super::{FullscreenController, ResourceWatchState};

impl FullscreenController {
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

        if let Some(install) = parse_install_command(text) {
            match install {
                InstallSlashAction::Help => {
                    self.send_command(FullscreenCommand::PushNote {
                        level: FullscreenNoteLevel::Status,
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

        match dispatch_local_slash_command(self, text) {
            Ok(handled) => Ok(handled),
            Err(err) => {
                tracing::error!("local command error: {err}");
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Error,
                    text: format!("Command error: {err}"),
                });
                Ok(true)
            }
        }
    }

    async fn check_for_updates_now(&mut self) -> Result<()> {
        if self.streaming {
            self.send_command(FullscreenCommand::SetStatusLine(
                "Cannot check for updates while a turn is running".to_string(),
            ));
            return Ok(());
        }

        self.send_command(FullscreenCommand::SetStatusLine(
            "Checking for updates...".to_string(),
        ));
        match update_check::check_for_updates(true, &self.session_setup.tool_ctx.cwd).await? {
            UpdateCheckOutcome::Disabled => {
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Update check is not configured yet".to_string(),
                ));
            }
            UpdateCheckOutcome::UpToDate => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Already up to date (v{})",
                    env!("CARGO_PKG_VERSION")
                )));
            }
            UpdateCheckOutcome::UpdateAvailable(notice) => {
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Status,
                    text: update_check::build_update_available_note(&notice),
                });
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Update available: {}",
                    notice.latest_version
                )));
            }
        }
        Ok(())
    }

    async fn install_and_reload_package(&mut self, local: bool, source: String) -> Result<()> {
        if self.streaming {
            self.send_command(FullscreenCommand::SetStatusLine(
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
        self.send_command(FullscreenCommand::SetStatusLine(format!(
            "Installing {source}..."
        )));

        let source_for_install = source.clone();
        tokio::task::spawn_blocking(move || install_package(&source_for_install, scope, &cwd))
            .await
            .map_err(|err| anyhow!("install task failed: {err}"))??;

        self.reload_runtime_resources().await?;
        self.send_command(FullscreenCommand::SetStatusLine(format!(
            "Installed {source} and reloaded resources"
        )));
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
        self.send_command(FullscreenCommand::SetStatusLine(
            "Detected package/settings change and reloaded resources".to_string(),
        ));
        Ok(())
    }

    pub(crate) async fn reload_runtime_resources(&mut self) -> Result<()> {
        if self.streaming {
            self.send_command(FullscreenCommand::SetStatusLine(
                "Cannot reload resources while a turn is running".to_string(),
            ));
            return Ok(());
        }

        let cwd = self.session_setup.tool_ctx.cwd.clone();
        self.send_command(FullscreenCommand::SetStatusLine(
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
            mut tools,
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

        let mut all_tools = builtin_tools();
        all_tools.append(&mut tools);
        self.session_setup.tool_defs = build_tool_defs(&all_tools);
        self.session_setup.tools = all_tools;
        self.session_setup.extension_commands = commands;
        self.session_setup.system_prompt = format!(
            "{}{}",
            self.session_setup.base_system_prompt,
            build_skill_system_prompt_section(&session_resources)
        );

        self.runtime_host.reload_resources(session_resources);
        self.runtime_host.runtime_mut().model = Some(RuntimeModelRef {
            provider: self.session_setup.model.provider.clone(),
            id: self.session_setup.model.id.clone(),
            context_window: self.session_setup.model.context_window as usize,
        });
        self.resource_watch = ResourceWatchState::capture(&self.session_setup.tool_ctx.cwd);
        self.send_command(FullscreenCommand::SetExtraSlashItems(
            build_dynamic_slash_items(&self.runtime_host),
        ));
        self.publish_footer();
        Ok(())
    }
}
