use anyhow::Result;
use bb_core::agent_session::{ModelRef, ThinkingLevel, parse_model_arg};
use bb_core::agent_session_runtime::RuntimeModelRef;
use bb_core::settings::{ExecutionMode, Settings};
use bb_core::types::AgentMessage;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, Model, ModelRegistry};
use bb_session::{context, store};
use bb_tui::fullscreen::{
    FullscreenAuthDialog, FullscreenAuthStep, FullscreenAuthStepState, FullscreenCommand,
    FullscreenNoteLevel,
};
use bb_tui::select_list::SelectItem;

use crate::slash::LocalSlashCommandHost;

use super::controller::FullscreenController;
use super::formatting::format_assistant_text;
use super::{
    FORK_ENTRY_MENU_ID, LOGIN_METHOD_MENU_ID, LOGIN_PROVIDER_MENU_ID, LOGIN_PROVIDERS,
    LOGOUT_PROVIDER_MENU_ID, RESUME_SESSION_MENU_ID, TREE_ENTRY_MENU_ID, TREE_SUMMARY_MENU_ID,
    copy_text_to_clipboard,
};

mod auth;
mod models;
mod settings;

impl FullscreenController {
    pub(super) async fn handle_menu_selection(
        &mut self,
        menu_id: &str,
        value: &str,
        submission_rx: &mut tokio::sync::mpsc::UnboundedReceiver<
            bb_tui::fullscreen::FullscreenSubmission,
        >,
    ) -> Result<()> {
        match menu_id {
            "model" => {
                if let Some((model, thinking)) = self.find_exact_model_match(value) {
                    self.apply_model_selection(model, thinking);
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Model not found: {value}"
                    )));
                }
            }
            "settings" => self.open_setting_values_menu(value),
            RESUME_SESSION_MENU_ID => self.handle_resume_session(value)?,
            TREE_ENTRY_MENU_ID => self.open_tree_summary_menu(value)?,
            TREE_SUMMARY_MENU_ID => {
                self.handle_tree_summary_selection(value, submission_rx)
                    .await?
            }
            FORK_ENTRY_MENU_ID => self.handle_fork_from_entry(value)?,
            LOGIN_PROVIDER_MENU_ID => {
                self.open_login_method_menu(value);
            }
            LOGIN_METHOD_MENU_ID => {
                if let Some(provider) = value.strip_prefix("oauth:") {
                    self.begin_oauth_login(provider, submission_rx).await?;
                } else if let Some(provider) = value.strip_prefix("api_key:") {
                    self.begin_api_key_login(provider);
                } else if value == "copilot:github" {
                    self.finish_copilot_host_setup("github.com")?;
                    self.begin_oauth_login("github-copilot", submission_rx)
                        .await?;
                } else if value == "copilot:enterprise" {
                    self.begin_copilot_enterprise_login();
                } else {
                    self.send_command(FullscreenCommand::PushNote {
                        level: FullscreenNoteLevel::Error,
                        text: format!("Unknown login method selection: {value}"),
                    });
                }
            }
            LOGOUT_PROVIDER_MENU_ID => {
                if crate::login::remove_auth(value)? {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Logged out of {value}"
                    )));
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "No saved credentials for {value}"
                    )));
                }
            }
            _ if menu_id.starts_with("settings:") => {
                let setting_id = menu_id.trim_start_matches("settings:");
                self.apply_setting_value(setting_id, value)?;
            }
            _ if menu_id.starts_with("ext:") => {
                if let Some(command) = self.pending_extension_menus.remove(menu_id) {
                    let invocation = if value.trim().is_empty() {
                        format!("/{command}")
                    } else {
                        format!("/{command} {value}")
                    };
                    self.execute_extension_command_text(&invocation).await?;
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Stale extension menu: {menu_id}"
                    )));
                }
            }
            _ => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Unknown fullscreen menu: {menu_id}"
                )));
            }
        }
        Ok(())
    }
}

impl LocalSlashCommandHost for FullscreenController {
    fn slash_help(&mut self) -> Result<()> {
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Status,
            text: crate::slash::help_lines().join("\n"),
        });
        Ok(())
    }

    fn slash_exit(&mut self) -> Result<()> {
        self.shutdown_requested = true;
        self.abort_token.cancel();
        Ok(())
    }

    fn slash_new_session(&mut self) -> Result<()> {
        self.handle_new_session();
        Ok(())
    }

    fn slash_compact(&mut self, _instructions: Option<&str>) -> Result<()> {
        // `/compact` is handled asynchronously in `handle_local_submission`
        // before shared local slash dispatch runs.
        self.send_command(FullscreenCommand::SetStatusLine(
            "Running /compact...".to_string(),
        ));
        Ok(())
    }

    fn slash_model_select(&mut self, search: Option<&str>) -> Result<()> {
        self.handle_model_selection_command(search)
    }

    fn slash_resume(&mut self) -> Result<()> {
        self.open_resume_menu()
    }

    fn slash_tree(&mut self) -> Result<()> {
        self.open_tree_menu(None)
    }

    fn slash_fork(&mut self) -> Result<()> {
        self.open_fork_menu()
    }

    fn slash_login(&mut self) -> Result<()> {
        self.open_login_provider_menu();
        Ok(())
    }

    fn slash_logout(&mut self) -> Result<()> {
        self.open_logout_provider_menu();
        Ok(())
    }

    fn slash_name(&mut self, name: Option<&str>) -> Result<()> {
        match name {
            Some(name) => {
                self.ensure_session_row_created()?;
                store::set_session_name(
                    &self.session_setup.conn,
                    &self.session_setup.session_id,
                    Some(name),
                )?;
                self.publish_footer();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Session name: {name}"
                )));
            }
            None => {
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Usage: /name <session name>".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn slash_session_info(&mut self) -> Result<()> {
        let summary = crate::session_info::collect_session_info_summary(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &self.session_setup.model.provider,
            &self.session_setup.model.id,
            &self.session_setup.thinking_level,
            self.session_setup.tool_ctx.execution_policy,
        )?;
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Status,
            text: crate::session_info::render_session_info_text(&summary),
        });
        Ok(())
    }

    fn slash_copy(&mut self) -> Result<()> {
        self.copy_last_assistant_message()
    }

    fn slash_settings(&mut self) -> Result<()> {
        self.open_settings_menu();
        Ok(())
    }

    fn slash_hotkeys(&mut self) -> Result<()> {
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Status,
            text: [
                "Keyboard Shortcuts",
                "  Ctrl+C          Interrupt / quit",
                "  Ctrl+O          Toggle transcript mode",
                "  Esc             Exit transcript / quit",
                "  Enter           Submit prompt",
                "  Shift+Enter     Insert newline",
                "  Ctrl+J          Submit prompt (alt)",
                "  /               Open command menu",
                "  !command        Run bash command",
                "",
                "Transcript Mode (Ctrl+O)",
                "  j/k             Navigate blocks",
                "  Enter/Space     Toggle expand/collapse",
                "  o               Expand focused block",
                "  c               Collapse focused block",
                "  g/G             Jump to first/last",
                "  Ctrl+O          Toggle tool output",
                "  Esc             Return to input",
            ]
            .join("\n"),
        });
        Ok(())
    }

    fn slash_reload(&mut self) -> Result<()> {
        self.send_command(FullscreenCommand::SetStatusLine(
            "Reload not yet supported in fullscreen mode. Use /quit and restart.".to_string(),
        ));
        Ok(())
    }

    fn slash_export(&mut self, path: Option<&str>) -> Result<()> {
        let file_path = path.unwrap_or("session-export.jsonl").to_string();
        match crate::fullscreen::session::export_session(
            &self.session_setup.conn,
            &self.session_setup.session_id,
            &file_path,
        ) {
            Ok(abs_path) => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Exported to: {abs_path}"
                )));
            }
            Err(e) => {
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Error,
                    text: format!("Export failed: {e}"),
                });
            }
        }
        Ok(())
    }

    fn slash_import(&mut self, path: Option<&str>) -> Result<()> {
        let Some(path) = path else {
            self.send_command(FullscreenCommand::SetStatusLine(
                "Usage: /import <path.jsonl>".to_string(),
            ));
            return Ok(());
        };
        self.send_command(FullscreenCommand::SetStatusLine(format!(
            "Import from {path} not yet supported in fullscreen mode."
        )));
        Ok(())
    }

    fn slash_image(&mut self, path: &str) -> Result<()> {
        use base64::Engine;

        let resolved = if std::path::Path::new(path).is_absolute() {
            std::path::PathBuf::from(path)
        } else {
            self.session_setup.tool_ctx.cwd.join(path)
        };

        // Read and validate the file
        let data = match std::fs::read(&resolved) {
            Ok(d) => d,
            Err(e) => {
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Error,
                    text: format!("Cannot read image: {e}"),
                });
                return Ok(());
            }
        };

        // Detect MIME type from extension
        let mime_type = match resolved
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref()
        {
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => {
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Error,
                    text: "Unsupported image format. Use png, jpg, gif, or webp.".to_string(),
                });
                return Ok(());
            }
        };

        let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
        let display_path = super::shorten_path(path);
        let size_kb = data.len() / 1024;

        self.pending_images.push(super::controller::PendingImage {
            data: encoded,
            mime_type: mime_type.to_string(),
        });

        let count = self.pending_images.len();
        self.send_command(FullscreenCommand::SetStatusLine(format!(
                "📎 {display_path} ({size_kb}KB, {mime_type}) attached — {count} image(s) pending. Type your prompt and press Enter."
            )));
        Ok(())
    }
}
