use anyhow::Result;
use bb_core::agent_session::{ModelRef, ThinkingLevel, parse_model_arg};
use bb_core::agent_session_runtime::RuntimeModelRef;
use bb_core::settings::Settings;
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

fn fullscreen_auth_method_label(provider: &str) -> &'static str {
    crate::login::provider_auth_method(provider)
}

fn fullscreen_auth_display_name(provider: &str) -> String {
    crate::login::provider_display_name(provider)
}

fn fullscreen_auth_status_detail(provider: &str) -> String {
    let base = match crate::login::auth_source(provider) {
        Some(_) => format!("({}) [configured]", fullscreen_auth_method_label(provider)),
        None => format!(
            "({}) [not authenticated]",
            fullscreen_auth_method_label(provider)
        ),
    };
    if provider == "github-copilot"
        && let Some(domain) = crate::login::github_copilot_domain()
    {
        return format!("{base} • host: {domain}");
    }
    base
}

#[derive(Default)]
struct NormalizedModelSelection {
    provider_filter: Option<String>,
    match_term: String,
    thinking_override: Option<ThinkingLevel>,
}

#[derive(Clone, Copy)]
enum OAuthDialogStage {
    Preparing,
    WaitingForBrowser,
    ProcessingCallback,
    ExchangingTokens,
}

fn auth_step(label: &str, state: FullscreenAuthStepState) -> FullscreenAuthStep {
    FullscreenAuthStep {
        label: label.to_string(),
        state: Some(state),
    }
}

fn build_oauth_dialog(
    label: &str,
    status: &str,
    stage: OAuthDialogStage,
    url: Option<String>,
    launcher_hint: Option<String>,
) -> FullscreenAuthDialog {
    let steps = match stage {
        OAuthDialogStage::Preparing => vec![
            auth_step("Open sign-in page", FullscreenAuthStepState::Active),
            auth_step(
                "Complete sign-in in your browser",
                FullscreenAuthStepState::Pending,
            ),
            auth_step(
                "Return via localhost callback or paste it here",
                FullscreenAuthStepState::Pending,
            ),
            auth_step("Save credentials", FullscreenAuthStepState::Pending),
        ],
        OAuthDialogStage::WaitingForBrowser => vec![
            auth_step("Open sign-in page", FullscreenAuthStepState::Done),
            auth_step(
                "Complete sign-in in your browser",
                FullscreenAuthStepState::Active,
            ),
            auth_step(
                "Return via localhost callback or paste it here",
                FullscreenAuthStepState::Pending,
            ),
            auth_step("Save credentials", FullscreenAuthStepState::Pending),
        ],
        OAuthDialogStage::ProcessingCallback => vec![
            auth_step("Open sign-in page", FullscreenAuthStepState::Done),
            auth_step(
                "Complete sign-in in your browser",
                FullscreenAuthStepState::Done,
            ),
            auth_step(
                "Return via localhost callback or paste it here",
                FullscreenAuthStepState::Active,
            ),
            auth_step("Save credentials", FullscreenAuthStepState::Pending),
        ],
        OAuthDialogStage::ExchangingTokens => vec![
            auth_step("Open sign-in page", FullscreenAuthStepState::Done),
            auth_step(
                "Complete sign-in in your browser",
                FullscreenAuthStepState::Done,
            ),
            auth_step(
                "Return via localhost callback or paste it here",
                FullscreenAuthStepState::Done,
            ),
            auth_step("Save credentials", FullscreenAuthStepState::Active),
        ],
    };

    let mut lines = Vec::new();
    if let Some(hint) = launcher_hint {
        lines.push(hint);
    }
    if url.is_some() {
        lines.push(
            "If the browser opens on another machine, paste the full localhost callback URL below and press Enter."
                .to_string(),
        );
    } else {
        lines.push("The authorization URL will appear below.".to_string());
    }

    FullscreenAuthDialog {
        title: format!("Sign in to {label}"),
        status: Some(status.to_string()),
        steps,
        url,
        lines,
        input_label: Some("Localhost callback URL".to_string()),
        input_placeholder: Some("Paste full localhost callback URL here".to_string()),
    }
}

fn build_device_oauth_dialog(
    label: &str,
    status: &str,
    verification_uri: String,
    user_code: String,
) -> FullscreenAuthDialog {
    FullscreenAuthDialog {
        title: format!("Sign in to {label}"),
        status: Some(status.to_string()),
        steps: vec![
            auth_step("Open verification page", FullscreenAuthStepState::Done),
            auth_step(
                "Enter device code in your browser",
                FullscreenAuthStepState::Active,
            ),
            auth_step(
                "Wait for bb to receive credentials",
                FullscreenAuthStepState::Pending,
            ),
        ],
        url: Some(verification_uri),
        lines: vec![
            format!("Device code: {user_code}"),
            "A future bb release will poll and exchange Copilot device tokens here.".to_string(),
        ],
        input_label: None,
        input_placeholder: None,
    }
}

fn build_copilot_enterprise_dialog() -> FullscreenAuthDialog {
    FullscreenAuthDialog {
        title: "GitHub Copilot Enterprise".to_string(),
        status: Some("Enter your GitHub Enterprise Server domain".to_string()),
        steps: vec![
            auth_step("Choose GitHub Enterprise Server host", FullscreenAuthStepState::Active),
            auth_step("Store host configuration", FullscreenAuthStepState::Pending),
            auth_step("Start Copilot OAuth/device flow", FullscreenAuthStepState::Pending),
        ],
        url: None,
        lines: vec![
            "Examples: github.acme.com or https://github.acme.com".to_string(),
            "Press Esc to cancel. Press Enter to save the host target, then bb will open the Copilot auth skeleton."
                .to_string(),
        ],
        input_label: Some("GitHub Enterprise Server domain".to_string()),
        input_placeholder: Some("github.example.com".to_string()),
    }
}

fn build_api_key_dialog(label: &str, url: &str) -> FullscreenAuthDialog {
    FullscreenAuthDialog {
        title: format!("Sign in to {label}"),
        status: Some("Paste your API key to continue".to_string()),
        steps: vec![
            auth_step("Open API key page if needed", FullscreenAuthStepState::Done),
            auth_step("Paste API key", FullscreenAuthStepState::Active),
            auth_step("Save credentials", FullscreenAuthStepState::Pending),
        ],
        url: Some(url.to_string()),
        lines: vec!["Your input stays local and will be stored in auth.json.".to_string()],
        input_label: Some("API key".to_string()),
        input_placeholder: Some("Paste API key here".to_string()),
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
        if let Some((model, thinking)) = self.find_exact_model_match(search_term) {
            self.apply_model_selection(model, thinking);
            return Ok(());
        }
        if let Some((model, thinking)) = self.find_unique_model_match(search_term) {
            self.apply_model_selection(model, thinking);
            return Ok(());
        }

        let normalized = self.normalize_model_selection(search_term);
        let needle = normalized.match_term.to_ascii_lowercase();
        let mut items: Vec<SelectItem> = self
            .get_model_candidates()
            .into_iter()
            .filter(|model| {
                if let Some(provider) = normalized.provider_filter.as_deref()
                    && model.provider != provider
                {
                    return false;
                }
                if needle.is_empty() {
                    true
                } else {
                    let provider_id =
                        format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
                    let provider_colon_id =
                        format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
                    provider_id.contains(&needle)
                        || provider_colon_id.contains(&needle)
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

    fn apply_model_selection(&mut self, model: Model, thinking_override: Option<ThinkingLevel>) {
        let api_key = crate::login::resolve_api_key(&model.provider).unwrap_or_default();
        let base_url = if model.provider == "github-copilot" {
            crate::login::github_copilot_api_base_url()
        } else {
            model.base_url.clone().unwrap_or_else(|| match model.api {
                ApiType::AnthropicMessages => "https://api.anthropic.com".to_string(),
                ApiType::GoogleGenerative => {
                    "https://generativelanguage.googleapis.com".to_string()
                }
                _ => "https://api.openai.com/v1".to_string(),
            })
        };
        let headers = if model.provider == "github-copilot" {
            crate::login::github_copilot_runtime_headers()
        } else {
            std::collections::HashMap::new()
        };
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
        if let Some(level) = thinking_override {
            self.session_setup.thinking_level = level.as_str().to_string();
            self.runtime_host.session_mut().set_thinking_level(level);
        }
        self.runtime_host.runtime_mut().model = Some(RuntimeModelRef {
            provider: model.provider.clone(),
            id: model.id.clone(),
            context_window: model.context_window as usize,
        });
        self.session_setup.model = model;
        self.session_setup.provider = new_provider;
        self.session_setup.api_key = api_key;
        self.session_setup.base_url = base_url;
        self.session_setup.headers = headers.clone();
        self.session_setup.tool_ctx.web_search = Some(bb_tools::WebSearchRuntime {
            provider: self.session_setup.provider.clone(),
            model: self.session_setup.model.clone(),
            api_key: self.session_setup.api_key.clone(),
            base_url: self.session_setup.base_url.clone(),
            headers,
            enabled: true,
        });
        let status = if let Some(level) = thinking_override {
            format!("Model: {display} • thinking: {}", level.as_str())
        } else {
            format!("Model: {display}")
        };
        self.options.model_display = Some(display);
        self.publish_footer();
        self.send_command(FullscreenCommand::SetStatusLine(status));
    }

    fn get_model_candidates(&self) -> Vec<Model> {
        let current_provider = self.session_setup.model.provider.clone();
        let available = crate::login::authenticated_providers();
        let mut registry = ModelRegistry::new();
        registry.load_custom_models(&Settings::load_merged(&self.session_setup.tool_ctx.cwd));
        for model_id in crate::login::github_copilot_cached_models() {
            if registry.find("github-copilot", &model_id).is_none() {
                registry.add(Model {
                    id: model_id.clone(),
                    name: model_id.clone(),
                    provider: "github-copilot".to_string(),
                    api: ApiType::OpenaiCompletions,
                    context_window: 128_000,
                    max_tokens: 16_384,
                    reasoning: true,
                    base_url: Some(crate::login::github_copilot_api_base_url()),
                    cost: Default::default(),
                });
            }
        }
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

    fn find_exact_model_match(&self, search_term: &str) -> Option<(Model, Option<ThinkingLevel>)> {
        let normalized = self.normalize_model_selection(search_term);
        let needle = normalized.match_term.to_ascii_lowercase();
        self.get_model_candidates().into_iter().find_map(|model| {
            if let Some(provider) = normalized.provider_filter.as_deref()
                && model.provider != provider
            {
                return None;
            }
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            let matched = model.id.eq_ignore_ascii_case(&needle)
                || model.name.eq_ignore_ascii_case(&needle)
                || provider_id == needle
                || provider_colon_id == needle;
            matched.then_some((model, normalized.thinking_override))
        })
    }

    fn find_unique_model_match(&self, search_term: &str) -> Option<(Model, Option<ThinkingLevel>)> {
        let normalized = self.normalize_model_selection(search_term);
        if normalized.match_term.is_empty() {
            return None;
        }
        let needle = normalized.match_term.to_ascii_lowercase();
        let mut matches = self.get_model_candidates().into_iter().filter(|model| {
            if let Some(provider) = normalized.provider_filter.as_deref()
                && model.provider != provider
            {
                return false;
            }
            let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
            let provider_colon_id = format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
            provider_id.contains(&needle)
                || provider_colon_id.contains(&needle)
                || model.id.to_ascii_lowercase().contains(&needle)
                || model.name.to_ascii_lowercase().contains(&needle)
        });
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some((first, normalized.thinking_override))
    }

    fn normalize_model_selection(&self, search_term: &str) -> NormalizedModelSelection {
        let search_term = search_term.trim();
        if search_term.is_empty() {
            return NormalizedModelSelection::default();
        }

        let current_provider = self.session_setup.model.provider.as_str();
        let (parsed_provider, parsed_model, thinking_override) =
            parse_model_arg(Some(current_provider), Some(search_term));
        let thinking_override = thinking_override.as_deref().and_then(ThinkingLevel::parse);

        if search_term.contains('/') {
            return NormalizedModelSelection {
                provider_filter: Some(parsed_provider),
                match_term: parsed_model,
                thinking_override,
            };
        }

        if let Some((provider, model)) = search_term.split_once(':')
            && !provider.is_empty()
            && !model.is_empty()
            && self
                .get_model_candidates()
                .iter()
                .any(|candidate| candidate.provider.eq_ignore_ascii_case(provider))
        {
            return NormalizedModelSelection {
                provider_filter: Some(provider.to_string()),
                match_term: model.to_string(),
                thinking_override,
            };
        }

        NormalizedModelSelection {
            provider_filter: None,
            match_term: if parsed_provider == current_provider {
                parsed_model
            } else {
                search_term.to_string()
            },
            thinking_override,
        }
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
            title: "Sign in provider".to_string(),
            items: LOGIN_PROVIDERS
                .iter()
                .map(|provider| {
                    let methods = match *provider {
                        "anthropic" | "openai" => "OAuth + API key",
                        "github-copilot" => "OAuth",
                        _ => "API key",
                    };
                    SelectItem {
                        label: match *provider {
                            "anthropic" => "Anthropic".to_string(),
                            "openai" => "OpenAI".to_string(),
                            "github-copilot" => "GitHub Copilot".to_string(),
                            "google" => "Google Gemini".to_string(),
                            "groq" => "Groq".to_string(),
                            "xai" => "xAI".to_string(),
                            "openrouter" => "OpenRouter".to_string(),
                            _ => (*provider).to_string(),
                        },
                        detail: Some(format!(
                            "{methods} • {}",
                            fullscreen_auth_status_detail(provider)
                        )),
                        value: (*provider).to_string(),
                    }
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
            "github-copilot" => {
                items.push(SelectItem {
                    label: "GitHub.com".to_string(),
                    detail: Some("Use the default github.com Copilot authority".to_string()),
                    value: "copilot:github".to_string(),
                });
                items.push(SelectItem {
                    label: "GitHub Enterprise Server".to_string(),
                    detail: Some("Enter your GitHub Enterprise Server domain".to_string()),
                    value: "copilot:enterprise".to_string(),
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
                "Sign in method: {}",
                match provider {
                    "anthropic" => "Anthropic",
                    "openai" => "OpenAI",
                    "github-copilot" => "GitHub Copilot",
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

    pub(super) async fn begin_oauth_login(
        &mut self,
        provider: &str,
        submission_rx: &mut tokio::sync::mpsc::UnboundedReceiver<
            bb_tui::fullscreen::FullscreenSubmission,
        >,
    ) -> Result<()> {
        use crate::oauth::OAuthCallbacks;
        use bb_tui::fullscreen::FullscreenSubmission;
        use std::sync::{Arc, Mutex};
        use tokio::sync::oneshot;

        let provider = crate::login::provider_oauth_variant(provider).unwrap_or(provider);
        let label = crate::login::provider_display_name(provider);
        let (manual_tx, manual_rx) = oneshot::channel::<String>();
        let mut manual_tx = Some(manual_tx);
        let dialog_shared = Arc::new(Mutex::new((None::<String>, None::<String>)));

        self.send_command(FullscreenCommand::SetLocalActionActive(true));
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.send_command(FullscreenCommand::OpenAuthDialog(build_oauth_dialog(
            &label,
            "Starting browser sign-in…",
            OAuthDialogStage::Preparing,
            None,
            None,
        )));
        self.send_command(FullscreenCommand::SetStatusLine(format!(
            "Starting OAuth login for {label}..."
        )));

        let command_tx = self.command_tx.clone();
        let label_for_auth = label.clone();
        let callbacks = OAuthCallbacks {
            on_auth: Box::new({
                let dialog_shared = dialog_shared.clone();
                move |url: String| {
                    let opened = crate::login::try_open_browser(&url);
                    let launcher_hint = if opened {
                        "A browser should open locally."
                    } else {
                        "No local browser launcher detected. Open the URL manually."
                    };
                    if let Ok(mut shared) = dialog_shared.lock() {
                        shared.0 = Some(url.clone());
                        shared.1 = Some(launcher_hint.to_string());
                    }
                    let _ =
                        command_tx.send(FullscreenCommand::UpdateAuthDialog(build_oauth_dialog(
                            &label_for_auth,
                            "Waiting for browser authentication…",
                            OAuthDialogStage::WaitingForBrowser,
                            Some(url),
                            Some(launcher_hint.to_string()),
                        )));
                }
            }),
            on_device_code: Some(Box::new({
                let command_tx = self.command_tx.clone();
                let label = label.clone();
                let dialog_shared = dialog_shared.clone();
                move |device| {
                    if let Ok(mut shared) = dialog_shared.lock() {
                        shared.0 = Some(device.verification_uri.clone());
                        shared.1 = Some(
                            "Open the verification URL and enter the device code.".to_string(),
                        );
                    }
                    let _ = command_tx.send(FullscreenCommand::SetStatusLine(format!(
                        "Enter device code {} in your browser",
                        device.user_code
                    )));
                    let _ = command_tx.send(FullscreenCommand::UpdateAuthDialog(
                        build_device_oauth_dialog(
                            &label,
                            "Complete device authentication in your browser…",
                            device.verification_uri,
                            device.user_code,
                        ),
                    ));
                }
            })),
            on_manual_input: Some(manual_rx),
            on_progress: Some(Box::new({
                let command_tx = self.command_tx.clone();
                let label = label.clone();
                let dialog_shared = dialog_shared.clone();
                move |msg: String| {
                    let (url, launcher_hint) = if let Ok(shared) = dialog_shared.lock() {
                        (shared.0.clone(), shared.1.clone())
                    } else {
                        (None, None)
                    };
                    let stage = if msg.contains("Exchanging authorization code") {
                        OAuthDialogStage::ExchangingTokens
                    } else {
                        OAuthDialogStage::WaitingForBrowser
                    };
                    let _ = command_tx.send(FullscreenCommand::SetStatusLine(msg.clone()));
                    let _ = command_tx.send(FullscreenCommand::UpdateAuthDialog(
                        build_oauth_dialog(&label, &msg, stage, url, launcher_hint),
                    ));
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
                                let (url, launcher_hint) = if let Ok(shared) = dialog_shared.lock() {
                                    (shared.0.clone(), shared.1.clone())
                                } else {
                                    (None, None)
                                };
                                self.send_command(FullscreenCommand::UpdateAuthDialog(
                                    build_oauth_dialog(
                                        &label,
                                        "Processing pasted callback…",
                                        OAuthDialogStage::ProcessingCallback,
                                        url,
                                        launcher_hint,
                                    ),
                                ));
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
                                let (url, launcher_hint) = if let Ok(shared) = dialog_shared.lock() {
                                    (shared.0.clone(), shared.1.clone())
                                } else {
                                    (None, None)
                                };
                                self.send_command(FullscreenCommand::UpdateAuthDialog(
                                    build_oauth_dialog(
                                        &label,
                                        "Processing pasted callback…",
                                        OAuthDialogStage::ProcessingCallback,
                                        url,
                                        launcher_hint,
                                    ),
                                ));
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
                self.send_command(FullscreenCommand::CloseAuthDialog);
                if cancelled {
                    self.send_command(FullscreenCommand::SetStatusLine(
                        "Authentication cancelled".to_string(),
                    ));
                } else if provider == "github-copilot" {
                    let model_count = crate::login::github_copilot_cached_models().len();
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Logged in to {} • refreshed {} models",
                        crate::login::provider_display_name(provider),
                        model_count
                    )));
                } else {
                    self.send_command(FullscreenCommand::SetStatusLine(format!(
                        "Logged in to {}",
                        crate::login::provider_display_name(provider)
                    )));
                }
                Ok(())
            }
            Err(err) => {
                self.send_command(FullscreenCommand::CloseAuthDialog);
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
        self.send_command(FullscreenCommand::OpenAuthDialog(build_api_key_dialog(
            &label, url,
        )));
        self.send_command(FullscreenCommand::SetStatusLine(format!(
            "Paste API key for {label} and press Enter"
        )));
    }

    fn begin_copilot_enterprise_login(&mut self) {
        self.pending_login_copilot_enterprise = true;
        self.send_command(FullscreenCommand::SetLocalActionActive(true));
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.send_command(FullscreenCommand::OpenAuthDialog(
            build_copilot_enterprise_dialog(),
        ));
        self.send_command(FullscreenCommand::SetStatusLine(
            "Enter your GitHub Enterprise Server domain".to_string(),
        ));
    }

    pub(super) fn finish_copilot_host_setup(&mut self, domain: &str) -> Result<()> {
        let domain = crate::login::normalize_github_domain(domain)?;
        crate::login::save_github_copilot_config(&domain)?;
        self.pending_login_copilot_enterprise = false;
        self.send_command(FullscreenCommand::CloseAuthDialog);
        self.send_command(FullscreenCommand::SetLocalActionActive(false));
        self.send_command(FullscreenCommand::SetInput(String::new()));
        self.send_command(FullscreenCommand::SetStatusLine(format!(
            "Saved GitHub Copilot host: {domain}"
        )));
        self.send_command(FullscreenCommand::PushNote {
            level: FullscreenNoteLevel::Status,
            text: format!(
                "GitHub Copilot authority configured for {domain}. bb will use this authority for the GitHub device flow and Copilot token exchange."
            ),
        });
        Ok(())
    }

    fn open_logout_provider_menu(&mut self) {
        let providers = crate::login::configured_providers();
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
