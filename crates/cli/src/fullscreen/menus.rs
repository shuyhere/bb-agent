use anyhow::Result;
use bb_core::agent_session::{ModelRef, ThinkingLevel};
use bb_core::agent_session_runtime::RuntimeModelRef;
use bb_core::settings::Settings;
use bb_core::types::AgentMessage;
use bb_provider::anthropic::AnthropicProvider;
use bb_provider::google::GoogleProvider;
use bb_provider::openai::OpenAiProvider;
use bb_provider::registry::{ApiType, Model, ModelRegistry};
use bb_session::{context, store};
use bb_tui::fullscreen::{FullscreenCommand, FullscreenNoteLevel};
use bb_tui::select_list::SelectItem;

use crate::slash::LocalSlashCommandHost;

use super::controller::FullscreenController;
use super::formatting::format_assistant_text;
use super::{
    copy_text_to_clipboard, FORK_ENTRY_MENU_ID, LOGIN_PROVIDERS, LOGIN_PROVIDER_MENU_ID,
    LOGOUT_PROVIDER_MENU_ID, OAUTH_PROVIDERS, RESUME_SESSION_MENU_ID, TREE_ENTRY_MENU_ID,
};

fn fullscreen_auth_method_label(provider: &str) -> &'static str {
    if OAUTH_PROVIDERS.contains(&provider) {
        "OAuth"
    } else {
        "API key"
    }
}

fn fullscreen_auth_display_name(provider: &str) -> String {
    match provider {
        "anthropic" => "Anthropic".to_string(),
        "openai-codex" => "OpenAI Codex".to_string(),
        "google" => "Google".to_string(),
        "groq" => "Groq".to_string(),
        "xai" => "xAI".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        _ => provider.to_string(),
    }
}

fn fullscreen_auth_status_detail(provider: &str) -> String {
    match crate::login::auth_source(provider) {
        Some(_) => format!("({}) [configured]", fullscreen_auth_method_label(provider)),
        None => format!(
            "({}) [not authenticated]",
            fullscreen_auth_method_label(provider)
        ),
    }
}

fn persist_fullscreen_retry_settings(
    enabled: bool,
    max_retries: u32,
    base_delay_ms: u64,
    max_delay_ms: u64,
) -> Result<()> {
    let mut settings = Settings::load_global();
    settings.retry.enabled = enabled;
    settings.retry.max_retries = max_retries.max(1);
    settings.retry.base_delay_ms = base_delay_ms.max(1_000);
    settings.retry.max_delay_ms = max_delay_ms.max(settings.retry.base_delay_ms);
    settings.save_global()?;
    Ok(())
}

impl FullscreenController {
    pub(super) async fn handle_menu_selection(
        &mut self,
        menu_id: &str,
        value: &str,
    ) -> Result<()> {
        match menu_id {
            "model" => {
                if let Some(model) = self.find_exact_model_match(value) {
                    self.apply_model_selection(model);
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Model not found: {value}"
                    )));
                }
            }
            "settings" => self.open_setting_values_menu(value),
            RESUME_SESSION_MENU_ID => self.handle_resume_session(value)?,
            TREE_ENTRY_MENU_ID => self.handle_tree_navigate(value)?,
            FORK_ENTRY_MENU_ID => self.handle_fork_from_entry(value)?,
            LOGIN_PROVIDER_MENU_ID => {
                let (_env_var, url) = crate::login::provider_meta(value);
                let mode = if OAUTH_PROVIDERS.contains(&value) {
                    "OAuth"
                } else {
                    "API key"
                };
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Status,
                    text: format!(
                        "Login provider: {value}\nMode: {mode}\nOpen: {url}\nUse `bb login {value}` in a normal terminal to complete authentication."
                    ),
                });
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
            _ => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Unknown fullscreen menu: {menu_id}"
                )));
            }
        }
        Ok(())
    }

    pub(super) fn handle_model_selection_command(
        &mut self,
        search: Option<&str>,
    ) -> Result<()> {
        let search_term = search.unwrap_or_default().trim();
        if let Some(model) = self.find_exact_model_match(search_term) {
            self.apply_model_selection(model);
            return Ok(());
        }

        let mut items: Vec<SelectItem> = self
            .get_model_candidates()
            .into_iter()
            .filter(|model| {
                if search_term.is_empty() {
                    true
                } else {
                    let needle = search_term.to_ascii_lowercase();
                    let provider_id =
                        format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
                    provider_id.contains(&needle)
                        || model.id.to_ascii_lowercase().contains(&needle)
                        || model.name.to_ascii_lowercase().contains(&needle)
                }
            })
            .map(|model| SelectItem {
                label: format!("{}/{}", model.provider, model.id),
                detail: Some(model.name.clone()),
                value: format!("{}/{}", model.provider, model.id),
            })
            .collect();
        items.sort_by(|a, b| a.label.cmp(&b.label));

        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: "model".to_string(),
            title: if search_term.is_empty() {
                "Select model".to_string()
            } else {
                format!("Select model matching '{search_term}'")
            },
            items,
        });
        Ok(())
    }

    fn open_settings_menu(&mut self) {
        let items = vec![
            SelectItem {
                label: format!("Thinking level [{}]", self.session_setup.thinking_level),
                detail: Some("Reasoning depth".to_string()),
                value: "thinking".to_string(),
            },
            SelectItem {
                label: format!(
                    "Auto-retry [{}]",
                    if self.session_setup.retry_enabled {
                        "true"
                    } else {
                        "false"
                    }
                ),
                detail: Some("Retry retryable provider errors".to_string()),
                value: "retry-enabled".to_string(),
            },
            SelectItem {
                label: format!("Retry attempts [{}]", self.session_setup.retry_max_retries),
                detail: Some("Maximum retry attempts".to_string()),
                value: "retry-max".to_string(),
            },
            SelectItem {
                label: format!(
                    "Retry base delay [{}s]",
                    self.session_setup.retry_base_delay_ms / 1000
                ),
                detail: Some("Initial retry backoff".to_string()),
                value: "retry-delay".to_string(),
            },
            SelectItem {
                label: format!(
                    "Retry max delay [{}s]",
                    self.session_setup.retry_max_delay_ms / 1000
                ),
                detail: Some("Maximum allowed retry delay".to_string()),
                value: "retry-max-delay".to_string(),
            },
        ];

        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: "settings".to_string(),
            title: "Settings".to_string(),
            items,
        });
    }

    fn open_setting_values_menu(&mut self, setting_id: &str) {
        let (title, values): (&str, Vec<&str>) = match setting_id {
            "thinking" => (
                "Thinking level",
                vec!["off", "low", "medium", "high", "xhigh"],
            ),
            "retry-enabled" => ("Auto-retry", vec!["true", "false"]),
            "retry-max" => ("Retry attempts", vec!["1", "2", "3", "4", "5"]),
            "retry-delay" => ("Retry base delay", vec!["1s", "2s", "5s", "10s"]),
            "retry-max-delay" => ("Retry max delay", vec!["10s", "30s", "60s", "120s"]),
            _ => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Unknown setting: {setting_id}"
                )));
                return;
            }
        };

        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: format!("settings:{setting_id}"),
            title: title.to_string(),
            items: values
                .into_iter()
                .map(|value| SelectItem {
                    label: value.to_string(),
                    detail: None,
                    value: value.to_string(),
                })
                .collect(),
        });
    }

    fn apply_setting_value(&mut self, setting_id: &str, value: &str) -> Result<()> {
        match setting_id {
            "thinking" => {
                self.session_setup.thinking_level = value.to_string();
                let level = match value {
                    "off" => ThinkingLevel::Off,
                    "low" | "minimal" => ThinkingLevel::Low,
                    "medium" => ThinkingLevel::Medium,
                    "high" => ThinkingLevel::High,
                    "xhigh" => ThinkingLevel::XHigh,
                    _ => ThinkingLevel::Medium,
                };
                self.runtime_host.session_mut().set_thinking_level(level);
                self.publish_footer();
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Thinking: {value}"
                )));
            }
            "retry-enabled" => {
                self.session_setup.retry_enabled = value == "true";
                persist_fullscreen_retry_settings(
                    self.session_setup.retry_enabled,
                    self.session_setup.retry_max_retries,
                    self.session_setup.retry_base_delay_ms,
                    self.session_setup.retry_max_delay_ms,
                )?;
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Auto-retry: {value}"
                )));
            }
            "retry-max" => {
                let parsed = value
                    .parse::<u32>()
                    .unwrap_or(self.session_setup.retry_max_retries);
                self.session_setup.retry_max_retries = parsed.max(1);
                persist_fullscreen_retry_settings(
                    self.session_setup.retry_enabled,
                    self.session_setup.retry_max_retries,
                    self.session_setup.retry_base_delay_ms,
                    self.session_setup.retry_max_delay_ms,
                )?;
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Retry attempts: {}",
                    self.session_setup.retry_max_retries
                )));
            }
            "retry-delay" => {
                let secs = value.trim_end_matches('s').parse::<u64>().unwrap_or(1);
                self.session_setup.retry_base_delay_ms = secs.max(1) * 1000;
                if self.session_setup.retry_max_delay_ms < self.session_setup.retry_base_delay_ms {
                    self.session_setup.retry_max_delay_ms = self.session_setup.retry_base_delay_ms;
                }
                persist_fullscreen_retry_settings(
                    self.session_setup.retry_enabled,
                    self.session_setup.retry_max_retries,
                    self.session_setup.retry_base_delay_ms,
                    self.session_setup.retry_max_delay_ms,
                )?;
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Retry base delay: {value}"
                )));
            }
            "retry-max-delay" => {
                let secs = value.trim_end_matches('s').parse::<u64>().unwrap_or(10);
                self.session_setup.retry_max_delay_ms = secs.max(1) * 1000;
                if self.session_setup.retry_max_delay_ms < self.session_setup.retry_base_delay_ms {
                    self.session_setup.retry_max_delay_ms = self.session_setup.retry_base_delay_ms;
                }
                persist_fullscreen_retry_settings(
                    self.session_setup.retry_enabled,
                    self.session_setup.retry_max_retries,
                    self.session_setup.retry_base_delay_ms,
                    self.session_setup.retry_max_delay_ms,
                )?;
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Retry max delay: {value}"
                )));
            }
            _ => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Unknown setting: {setting_id}"
                )));
            }
        }
        Ok(())
    }

    fn apply_model_selection(&mut self, model: Model) {
        let api_key = crate::login::resolve_api_key(&model.provider).unwrap_or_default();
        let base_url = model.base_url.clone().unwrap_or_else(|| match model.api {
            ApiType::AnthropicMessages => "https://api.anthropic.com".to_string(),
            ApiType::GoogleGenerative => {
                "https://generativelanguage.googleapis.com".to_string()
            }
            _ => "https://api.openai.com/v1".to_string(),
        });
        let new_provider: std::sync::Arc<dyn bb_provider::Provider> = match model.api {
            ApiType::AnthropicMessages => std::sync::Arc::new(AnthropicProvider::new()),
            ApiType::GoogleGenerative => std::sync::Arc::new(GoogleProvider::new()),
            _ => std::sync::Arc::new(OpenAiProvider::new()),
        };
        let display = format!("{}/{}", model.provider, model.id);

        self.runtime_host.session_mut().set_model(ModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            reasoning: model.reasoning,
        });
        self.runtime_host.runtime_mut().model = Some(RuntimeModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            context_window: model.context_window as usize,
        });
        self.session_setup.model = model;
        self.session_setup.provider = new_provider;
        self.session_setup.api_key = api_key;
        self.session_setup.base_url = base_url;
        let status = format!("Model: {display}");
        self.options.model_display = Some(display);
        self.publish_footer();
        self.send_command(FullscreenCommand::SetStatusLine(status));
    }

    fn get_model_candidates(&self) -> Vec<Model> {
        let current_provider = self.session_setup.model.provider.clone();
        let available = crate::login::authenticated_providers();
        let mut registry = ModelRegistry::new();
        registry.load_custom_models(&Settings::load_merged(&self.session_setup.tool_ctx.cwd));
        registry
            .list()
            .iter()
            .filter(|model| {
                available.iter().any(|provider| provider == &model.provider)
                    || model.provider == current_provider
            })
            .cloned()
            .collect()
    }

    fn find_exact_model_match(&self, search_term: &str) -> Option<Model> {
        let needle = search_term.trim().to_ascii_lowercase();
        self.get_model_candidates().into_iter().find(|model| {
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id =
                format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            model.id.eq_ignore_ascii_case(&needle)
                || model.name.eq_ignore_ascii_case(&needle)
                || provider_id == needle
                || provider_colon_id == needle
        })
    }

    fn copy_last_assistant_message(&mut self) -> Result<()> {
        let session_context = context::build_context(
            &self.session_setup.conn,
            &self.session_setup.session_id,
        )?;
        let last_text = session_context
            .messages
            .into_iter()
            .rev()
            .find_map(|message| match message {
                AgentMessage::Assistant(message) => {
                    let text = format_assistant_text(&message);
                    if text.trim().is_empty() {
                        None
                    } else {
                        Some(text)
                    }
                }
                _ => None,
            });

        if let Some(text) = last_text {
            copy_text_to_clipboard(&text)?;
            self.send_command(FullscreenCommand::SetStatusLine(
                "Copied last assistant message to clipboard".to_string(),
            ));
        } else {
            self.send_command(FullscreenCommand::SetStatusLine(
                "No assistant messages to copy".to_string(),
            ));
        }
        Ok(())
    }

    fn open_login_provider_menu(&mut self) {
        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: LOGIN_PROVIDER_MENU_ID.to_string(),
            title: "Login provider".to_string(),
            items: LOGIN_PROVIDERS
                .iter()
                .map(|provider| SelectItem {
                    label: fullscreen_auth_display_name(provider),
                    detail: Some(fullscreen_auth_status_detail(provider)),
                    value: (*provider).to_string(),
                })
                .collect(),
        });
    }

    fn open_logout_provider_menu(&mut self) {
        let providers = crate::login::authenticated_providers();
        if providers.is_empty() {
            self.send_command(FullscreenCommand::SetStatusLine(
                "No logged-in providers".to_string(),
            ));
            return;
        }
        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: LOGOUT_PROVIDER_MENU_ID.to_string(),
            title: "Logout provider".to_string(),
            items: providers
                .into_iter()
                .map(|provider| SelectItem {
                    label: fullscreen_auth_display_name(&provider),
                    detail: Some(fullscreen_auth_status_detail(&provider)),
                    value: provider,
                })
                .collect(),
        });
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

    fn slash_compact(&mut self, instructions: Option<&str>) -> Result<()> {
        self.handle_compact_command(instructions)
    }

    fn slash_model_select(&mut self, search: Option<&str>) -> Result<()> {
        self.handle_model_selection_command(search)
    }

    fn slash_resume(&mut self) -> Result<()> {
        self.open_resume_menu()
    }

    fn slash_tree(&mut self) -> Result<()> {
        self.open_tree_menu()
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
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Status,
            text: format!(
                "Session: {}\nModel: {}/{}\nThinking: {}",
                self.session_setup.session_id,
                self.session_setup.model.provider,
                self.session_setup.model.id,
                self.session_setup.thinking_level
            ),
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
                self.send_command(FullscreenCommand::SetStatusLine(
                    format!("Exported to: {abs_path}"),
                ));
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
        self.send_command(FullscreenCommand::SetStatusLine(
            format!("Import from {path} not yet supported in fullscreen mode."),
        ));
        Ok(())
    }
}
