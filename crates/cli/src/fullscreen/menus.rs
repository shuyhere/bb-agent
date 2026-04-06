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
    FORK_ENTRY_MENU_ID, LOGIN_METHOD_MENU_ID, LOGIN_PROVIDER_MENU_ID, LOGIN_PROVIDERS,
    LOGOUT_PROVIDER_MENU_ID, RESUME_SESSION_MENU_ID, TREE_ENTRY_MENU_ID, TREE_SUMMARY_MENU_ID,
    copy_text_to_clipboard,
};

fn fullscreen_auth_method_label(provider: &str) -> &'static str {
    crate::login::provider_auth_method(provider)
}

fn fullscreen_auth_display_name(provider: &str) -> String {
    crate::login::provider_display_name(provider)
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
        submission_rx: &mut tokio::sync::mpsc::UnboundedReceiver<
            bb_tui::fullscreen::FullscreenSubmission,
        >,
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
            _ => {
                self.send_command(FullscreenCommand::SetStatusLine(format!(
                    "Unknown fullscreen menu: {menu_id}"
                )));
            }
        }
        Ok(())
    }

    pub(super) fn handle_model_selection_command(&mut self, search: Option<&str>) -> Result<()> {
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
            selected_value: None,
        });
        Ok(())
    }

    fn current_color_theme_name(&self) -> &'static str {
        self.color_theme.name()
    }

    fn open_settings_menu(&mut self) {
        let items = vec![
            SelectItem {
                label: format!("Color theme [{}]", self.current_color_theme_name()),
                detail: Some("User input block & spinner colors".to_string()),
                value: "color-theme".to_string(),
            },
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
            selected_value: None,
        });
    }

    fn open_setting_values_menu(&mut self, setting_id: &str) {
        let (title, values): (&str, Vec<&str>) = match setting_id {
            "color-theme" => (
                "Color theme",
                vec!["pink", "lavender", "ocean", "mint", "sunset", "slate"],
            ),
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
            selected_value: None,
        });
    }

    fn apply_setting_value(&mut self, setting_id: &str, value: &str) -> Result<()> {
        match setting_id {
            "color-theme" => {
                if let Some(theme) = bb_tui::fullscreen::spinner::ColorTheme::from_name(value) {
                    self.color_theme = theme;
                    self.send_command(FullscreenCommand::SetColorTheme(theme));
                    // Persist to settings
                    let mut settings = bb_core::settings::Settings::load_global();
                    settings.color_theme = Some(value.to_string());
                    let _ = settings.save_global();
                    self.mark_local_settings_saved();
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Color theme: {value}"
                    )));
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Unknown color theme: {value}"
                    )));
                }
            }
            "thinking" => {
                let level = ThinkingLevel::parse(value).unwrap_or(ThinkingLevel::Medium);
                self.session_setup.thinking_level = level.as_str().to_string();
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
                self.mark_local_settings_saved();
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
                self.mark_local_settings_saved();
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
                self.mark_local_settings_saved();
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
                self.mark_local_settings_saved();
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
            ApiType::GoogleGenerative => "https://generativelanguage.googleapis.com".to_string(),
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
        self.session_setup.tool_ctx.web_search = Some(bb_tools::WebSearchRuntime {
            provider: self.session_setup.provider.clone(),
            model: self.session_setup.model.clone(),
            api_key: self.session_setup.api_key.clone(),
            base_url: self.session_setup.base_url.clone(),
            headers: std::collections::HashMap::new(),
            enabled: true,
        });
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
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            model.id.eq_ignore_ascii_case(&needle)
                || model.name.eq_ignore_ascii_case(&needle)
                || provider_id == needle
                || provider_colon_id == needle
        })
    }

    fn copy_last_assistant_message(&mut self) -> Result<()> {
        let session_context =
            context::build_context(&self.session_setup.conn, &self.session_setup.session_id)?;
        let last_text =
            session_context
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
                    label: match *provider {
                        "anthropic" => "Anthropic".to_string(),
                        "openai" => "OpenAI".to_string(),
                        "google" => "Google Gemini".to_string(),
                        "groq" => "Groq".to_string(),
                        "xai" => "xAI".to_string(),
                        "openrouter" => "OpenRouter".to_string(),
                        _ => (*provider).to_string(),
                    },
                    detail: Some(fullscreen_auth_status_detail(provider)),
                    value: (*provider).to_string(),
                })
                .collect(),
            selected_value: None,
        });
    }

    fn open_login_method_menu(&mut self, provider: &str) {
        let mut items = Vec::new();
        match provider {
            "anthropic" => {
                items.push(SelectItem {
                    label: "Claude Pro/Max".to_string(),
                    detail: Some("OAuth subscription login".to_string()),
                    value: "oauth:anthropic".to_string(),
                });
                items.push(SelectItem {
                    label: "Anthropic API key".to_string(),
                    detail: Some("Use ANTHROPIC_API_KEY or paste a key".to_string()),
                    value: "api_key:anthropic".to_string(),
                });
            }
            "openai" => {
                items.push(SelectItem {
                    label: "ChatGPT Plus/Pro (Codex)".to_string(),
                    detail: Some("OAuth subscription login".to_string()),
                    value: "oauth:openai-codex".to_string(),
                });
                items.push(SelectItem {
                    label: "OpenAI API key".to_string(),
                    detail: Some("Use OPENAI_API_KEY or paste a key".to_string()),
                    value: "api_key:openai".to_string(),
                });
            }
            "google" => {
                items.push(SelectItem {
                    label: "Google API key".to_string(),
                    detail: Some("Use GOOGLE_API_KEY or paste a key".to_string()),
                    value: "api_key:google".to_string(),
                });
            }
            "groq" => {
                items.push(SelectItem {
                    label: "Groq API key".to_string(),
                    detail: Some("Use GROQ_API_KEY or paste a key".to_string()),
                    value: "api_key:groq".to_string(),
                });
            }
            "xai" => {
                items.push(SelectItem {
                    label: "xAI API key".to_string(),
                    detail: Some("Use XAI_API_KEY or paste a key".to_string()),
                    value: "api_key:xai".to_string(),
                });
            }
            "openrouter" => {
                items.push(SelectItem {
                    label: "OpenRouter API key".to_string(),
                    detail: Some("Use OPENROUTER_API_KEY or paste a key".to_string()),
                    value: "api_key:openrouter".to_string(),
                });
            }
            _ => {}
        }

        self.send_command(FullscreenCommand::OpenSelectMenu {
            menu_id: LOGIN_METHOD_MENU_ID.to_string(),
            title: format!(
                "Login method: {}",
                match provider {
                    "anthropic" => "Anthropic",
                    "openai" => "OpenAI",
                    "google" => "Google Gemini",
                    "groq" => "Groq",
                    "xai" => "xAI",
                    "openrouter" => "OpenRouter",
                    _ => provider,
                }
            ),
            items,
            selected_value: None,
        });
    }

    async fn begin_oauth_login(
        &mut self,
        provider: &str,
        submission_rx: &mut tokio::sync::mpsc::UnboundedReceiver<
            bb_tui::fullscreen::FullscreenSubmission,
        >,
    ) -> Result<()> {
        use crate::oauth::OAuthCallbacks;
        use bb_tui::fullscreen::FullscreenSubmission;
        use tokio::sync::oneshot;

        let provider = crate::login::provider_oauth_variant(provider).unwrap_or(provider);
        let label = crate::login::provider_display_name(provider);
        let (manual_tx, manual_rx) = oneshot::channel::<String>();
        let mut manual_tx = Some(manual_tx);

        self.send_command(FullscreenCommand::SetLocalActionActive(true));
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.send_command(FullscreenCommand::SetStatusLine(format!(
            "Starting OAuth login for {label}..."
        )));

        let command_tx = self.command_tx.clone();
        let label_for_auth = label.clone();
        let callbacks = OAuthCallbacks {
            on_auth: Box::new(move |url: String| {
                let _ = command_tx.send(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Status,
                    text: format!(
                        "Use `/login` to sign in.\nProvider: {label_for_auth}\nMode: OAuth\nOpen: {url}\nIf the browser opens on another machine, paste the full localhost callback URL into the input box here and press Enter.\nPress Esc to cancel."
                    ),
                });
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&url).spawn();
                #[cfg(not(target_os = "macos"))]
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            }),
            on_manual_input: Some(manual_rx),
            on_progress: Some(Box::new({
                let command_tx = self.command_tx.clone();
                move |msg: String| {
                    let _ = command_tx.send(FullscreenCommand::SetStatusLine(msg));
                }
            })),
        };

        let login = crate::login::run_oauth_login(provider, callbacks);
        tokio::pin!(login);

        let mut cancelled = false;
        let outcome = loop {
            tokio::select! {
                maybe_submission = submission_rx.recv() => {
                    match maybe_submission {
                        Some(FullscreenSubmission::CancelLocalAction) => {
                            cancelled = true;
                            if let Some(tx) = manual_tx.take() {
                                let _ = tx.send(String::new());
                            }
                            break Ok::<_, anyhow::Error>(());
                        }
                        Some(FullscreenSubmission::Input(text)) => {
                            let text = text.trim().to_string();
                            if !text.is_empty()
                                && let Some(tx) = manual_tx.take()
                            {
                                let _ = tx.send(text);
                                self.send_command(FullscreenCommand::SetInput(String::new()));
                                self.send_command(FullscreenCommand::SetStatusLine(
                                    "Processing pasted callback...".to_string(),
                                ));
                            }
                        }
                        Some(FullscreenSubmission::InputWithImages { text, .. }) => {
                            let text = text.trim().to_string();
                            if !text.is_empty()
                                && let Some(tx) = manual_tx.take()
                            {
                                let _ = tx.send(text);
                                self.send_command(FullscreenCommand::SetInput(String::new()));
                                self.send_command(FullscreenCommand::SetStatusLine(
                                    "Processing pasted callback...".to_string(),
                                ));
                            }
                        }
                        Some(FullscreenSubmission::MenuSelection { .. }) => {}
                        None => {
                            cancelled = true;
                            if let Some(tx) = manual_tx.take() {
                                let _ = tx.send(String::new());
                            }
                            break Ok::<_, anyhow::Error>(());
                        }
                    }
                }
                result = &mut login => {
                    break result;
                }
            }
        };

        self.send_command(FullscreenCommand::SetLocalActionActive(false));
        match outcome {
            Ok(()) => {
                if cancelled {
                    self.send_command(FullscreenCommand::SetStatusLine(
                        "Authentication cancelled".to_string(),
                    ));
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Logged in to {}",
                        crate::login::provider_display_name(provider)
                    )));
                }
                Ok(())
            }
            Err(err) => {
                self.send_command(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Error,
                    text: format!("Authentication failed: {err}"),
                });
                self.send_command(FullscreenCommand::SetStatusLine(
                    "Authentication failed".to_string(),
                ));
                Ok(())
            }
        }
    }

    fn begin_api_key_login(&mut self, provider: &str) {
        let provider = crate::login::provider_api_key_variant(provider).unwrap_or(provider);
        self.pending_login_api_key_provider = Some(provider.to_string());
        let (_env_var, url) = crate::login::provider_meta(provider);
        let label = crate::login::provider_display_name(provider);
        self.send_command(FullscreenCommand::SetLocalActionActive(true));
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Status,
            text: format!(
                "Use `/login` to sign in.\nProvider: {label}\nMode: API key\nOpen: {url}\nPaste your API key into the input box below and press Enter.\nPress Esc to cancel."
            ),
        });
        self.send_command(FullscreenCommand::SetStatusLine(format!(
            "Paste API key for {label} and press Enter"
        )));
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
            selected_value: None,
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
