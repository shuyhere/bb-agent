use super::*;

#[derive(Default)]
struct NormalizedModelSelection {
    provider_filter: Option<String>,
    match_term: String,
    thinking_override: Option<ThinkingLevel>,
}

impl TuiController {
    pub(super) fn handle_model_selection_command(&mut self, search: Option<&str>) -> Result<()> {
        let search_term = search.unwrap_or_default().trim();
        let normalized = self.normalize_model_selection(search_term);
        if normalized.provider_filter.is_none() {
            let providers = self.matching_model_providers(search_term);
            if providers.len() > 1 {
                self.pending_model_provider_search = Some(search_term.to_string());
                self.send_command(TuiCommand::OpenSelectMenu {
                    menu_id: super::MODEL_PROVIDER_MENU_ID.to_string(),
                    title: format!("Select provider for '{search_term}'"),
                    items: providers
                        .into_iter()
                        .map(|provider| SelectItem {
                            label: crate::login::provider_display_name(&provider).into_owned(),
                            detail: Some(crate::login::provider_model_selection_detail(&provider)),
                            value: provider,
                        })
                        .collect(),
                    selected_value: None,
                });
                return Ok(());
            }
        }

        if let Some((model, thinking)) = self.find_exact_model_match(search_term) {
            return self.select_model_with_auth(model, thinking);
        }
        if let Some((model, thinking)) = self.find_unique_model_match(search_term) {
            return self.select_model_with_auth(model, thinking);
        }

        self.open_model_menu(search_term, normalized.provider_filter.as_deref())
    }

    pub(super) fn select_model_with_auth(
        &mut self,
        model: Model,
        thinking_override: Option<ThinkingLevel>,
    ) -> Result<()> {
        if self.maybe_open_model_auth_menu(model.clone(), thinking_override)? {
            return Ok(());
        }
        self.apply_model_selection(model, thinking_override);
        Ok(())
    }

    fn maybe_open_model_auth_menu(
        &mut self,
        model: Model,
        thinking_override: Option<ThinkingLevel>,
    ) -> Result<bool> {
        let options = crate::login::provider_auth_option_summaries(&model.provider);
        if options.len() <= 1 {
            return Ok(false);
        }

        self.pending_model_auth_selection =
            Some(crate::tui::controller::PendingModelAuthSelection {
                model: model.clone(),
                thinking_override,
            });
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: super::MODEL_AUTH_MENU_ID.to_string(),
            title: format!(
                "Select auth for {}",
                crate::login::provider_display_name(&model.provider)
            ),
            items: options
                .into_iter()
                .map(|option| {
                    let label = match (option.method, option.source) {
                        (
                            crate::login::ProviderAuthMethod::ApiKey,
                            crate::login::AuthSource::EnvVar,
                        ) => option
                            .account_label
                            .as_ref()
                            .map(|label| format!("API key (env) • {label}"))
                            .unwrap_or_else(|| "API key (env)".to_string()),
                        (
                            crate::login::ProviderAuthMethod::ApiKey,
                            crate::login::AuthSource::BbAuth,
                        ) => option
                            .account_label
                            .as_ref()
                            .map(|label| format!("API key • {label}"))
                            .unwrap_or_else(|| "API key".to_string()),
                        (
                            crate::login::ProviderAuthMethod::OAuth,
                            crate::login::AuthSource::EnvVar,
                        ) => option
                            .account_label
                            .as_ref()
                            .map(|label| format!("OAuth (env) • {label}"))
                            .unwrap_or_else(|| "OAuth (env)".to_string()),
                        (
                            crate::login::ProviderAuthMethod::OAuth,
                            crate::login::AuthSource::BbAuth,
                        ) => option
                            .account_label
                            .as_ref()
                            .map(|label| format!("OAuth • {label}"))
                            .unwrap_or_else(|| "OAuth".to_string()),
                    };
                    let mut detail_parts = Vec::new();
                    if option.active {
                        detail_parts.push("currently active".to_string());
                    }
                    if matches!(option.source, crate::login::AuthSource::BbAuth)
                        && matches!(option.method, crate::login::ProviderAuthMethod::ApiKey)
                        && let Some(profile_id) = option.profile_id.as_ref()
                    {
                        let suffix = profile_id
                            .chars()
                            .rev()
                            .take(6)
                            .collect::<Vec<_>>()
                            .into_iter()
                            .rev()
                            .collect::<String>();
                        detail_parts.push(format!("profile {suffix}"));
                    }
                    if let Some(authority) = option.authority {
                        detail_parts.push(authority);
                    }
                    if let Some(timestamp_ms) = option.configured_at_ms.or(option.updated_at_ms)
                        && let Some(dt) =
                            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms)
                    {
                        detail_parts.push(format!("saved {}", dt.format("%Y-%m-%d %H:%M UTC")));
                    }
                    SelectItem {
                        label,
                        detail: (!detail_parts.is_empty()).then_some(detail_parts.join(" • ")),
                        value: option
                            .profile_id
                            .map(|profile_id| format!("profile:{profile_id}"))
                            .unwrap_or_else(|| format!("env:{}", option.method.footer_label())),
                    }
                })
                .collect(),
            selected_value: None,
        });
        Ok(true)
    }

    pub(super) fn open_model_menu(
        &mut self,
        search_term: &str,
        provider_filter: Option<&str>,
    ) -> Result<()> {
        let normalized = self.normalize_model_selection(search_term);
        let provider_filter = provider_filter
            .map(ToString::to_string)
            .or(normalized.provider_filter);
        let needle = normalized.match_term.to_ascii_lowercase();
        let mut items: Vec<SelectItem> = self
            .get_model_candidates()
            .into_iter()
            .filter(|model| {
                if let Some(provider) = provider_filter.as_deref()
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

        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: "model".to_string(),
            title: if let Some(provider) = provider_filter.as_deref() {
                if search_term.is_empty() {
                    format!("Select model from {provider}")
                } else {
                    format!("Select {provider} model matching '{search_term}'")
                }
            } else if search_term.is_empty() {
                "Select model".to_string()
            } else {
                format!("Select model matching '{search_term}'")
            },
            items,
            selected_value: None,
        });
        Ok(())
    }

    pub(crate) fn maybe_switch_to_preferred_post_login_model(
        &mut self,
        provider: &str,
    ) -> Option<String> {
        let settings = Settings::load_merged(&self.session_setup.tool_ctx.cwd);
        let preferred_provider = match provider {
            "openai-codex" => "openai",
            other => other,
        };
        let preferred_model_id = if settings.default_provider.as_deref() == Some(preferred_provider)
            || (provider == "openai-codex"
                && settings.default_provider.as_deref() == Some("openai-codex"))
        {
            crate::login::available_model_for_provider(
                &settings,
                preferred_provider,
                settings.default_model.as_deref(),
            )?
        } else {
            crate::login::preferred_available_model_for_provider(&settings, preferred_provider)?
        };
        let mut registry = ModelRegistry::new();
        registry.load_custom_models(&settings);
        crate::login::add_cached_github_copilot_models(&mut registry);
        let model = registry
            .find(preferred_provider, &preferred_model_id)
            .cloned()
            .or_else(|| {
                registry
                    .find_fuzzy(&preferred_model_id, Some(preferred_provider))
                    .cloned()
            })?;
        let display = format!("{}/{}", model.provider, model.id);
        self.apply_model_selection(model, None);
        Some(display)
    }

    pub(super) fn apply_model_selection(
        &mut self,
        model: Model,
        thinking_override: Option<ThinkingLevel>,
    ) {
        let auth = crate::login::resolve_provider_auth(&model.provider);
        self.apply_model_selection_with_auth(model, thinking_override, auth);
    }

    pub(super) fn apply_model_selection_with_auth(
        &mut self,
        model: Model,
        thinking_override: Option<ThinkingLevel>,
        auth: Option<crate::login::ResolvedProviderAuth>,
    ) {
        let api_key = auth
            .as_ref()
            .map(|auth| auth.credential.clone())
            .unwrap_or_default();
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
        self.runtime_host
            .runtime_mut()
            .set_model(Some(RuntimeModelRef {
                provider: model.provider.clone(),
                id: model.id.clone(),
                context_window: model.context_window as usize,
            }));
        self.session_setup.model = model;
        self.session_setup.provider = new_provider;
        self.session_setup.auth = auth;
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
        if let Ok(mut tracker) = self.session_setup.request_metrics_tracker.try_lock() {
            tracker.reset_history();
        }
        self.publish_footer();
        self.send_command(TuiCommand::SetStatusLine(status));
    }

    pub(super) fn get_model_candidates(&self) -> Vec<Model> {
        let settings = Settings::load_merged(&self.session_setup.tool_ctx.cwd);
        crate::login::authenticated_model_candidates(&settings)
    }

    pub(super) fn find_exact_model_match(
        &self,
        search_term: &str,
    ) -> Option<(Model, Option<ThinkingLevel>)> {
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

    pub(super) fn find_unique_model_match(
        &self,
        search_term: &str,
    ) -> Option<(Model, Option<ThinkingLevel>)> {
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

    pub(super) fn matching_model_providers(&self, search_term: &str) -> Vec<String> {
        let normalized = self.normalize_model_selection(search_term);
        if normalized.provider_filter.is_some() || normalized.match_term.is_empty() {
            return Vec::new();
        }
        let needle = normalized.match_term.to_ascii_lowercase();
        let mut providers = self
            .get_model_candidates()
            .into_iter()
            .filter(|model| {
                let provider_id = format!("{}/{}", model.provider, model.id).to_ascii_lowercase();
                let provider_colon_id =
                    format!("{}:{}", model.provider, model.id).to_ascii_lowercase();
                provider_id.contains(&needle)
                    || provider_colon_id.contains(&needle)
                    || model.id.to_ascii_lowercase().contains(&needle)
                    || model.name.to_ascii_lowercase().contains(&needle)
            })
            .map(|model| model.provider)
            .collect::<Vec<_>>();
        providers.sort();
        providers.dedup();
        providers
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

    pub(super) fn copy_last_assistant_message(&mut self) -> Result<()> {
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
            self.send_command(TuiCommand::SetStatusLine(
                "Copied last assistant message to clipboard".to_string(),
            ));
        } else {
            self.send_command(TuiCommand::SetStatusLine(
                "No assistant messages to copy".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use bb_core::agent_session_runtime::{AgentSessionRuntimeBootstrap, AgentSessionRuntimeHost};
    use bb_core::settings::{ModelOverride, Settings};
    use bb_provider::openai::OpenAiProvider;
    use bb_provider::registry::{ApiType, CostConfig, Model, ModelInput};
    use bb_session::store;
    use bb_tools::{ExecutionPolicy, ToolContext, ToolExecutionMode};
    use bb_tui::tui::TuiCommand;
    use tokio::sync::mpsc;

    use crate::extensions::{ExtensionBootstrap, ExtensionCommandRegistry};
    use crate::session_bootstrap::{SessionRuntimeSetup, SessionUiOptions};
    use crate::tui::controller::TuiController;
    use crate::tui::{MODEL_AUTH_MENU_ID, MODEL_PROVIDER_MENU_ID};

    fn env_lock() -> &'static Mutex<()> {
        crate::login::auth_test_env_lock()
    }

    struct EnvVarGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let old = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, old }
        }

        fn set_value(key: &'static str, value: &str) -> Self {
            let old = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, old }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.old {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn test_model() -> Model {
        Model {
            id: "gpt-test".to_string(),
            name: "gpt-test".to_string(),
            provider: "openai".to_string(),
            api: ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16_384,
            reasoning: false,
            input: vec![ModelInput::Text],
            base_url: None,
            cost: CostConfig::default(),
        }
    }

    fn build_test_controller(
        cwd: std::path::PathBuf,
    ) -> (TuiController, mpsc::UnboundedReceiver<TuiCommand>) {
        let conn = store::open_memory().expect("memory db");
        let model = test_model();
        let tool_ctx = ToolContext {
            cwd: cwd.clone(),
            artifacts_dir: cwd.join("artifacts"),
            execution_policy: ExecutionPolicy::Safety,
            on_output: None,
            web_search: None,
            execution_mode: ToolExecutionMode::Interactive,
            request_approval: None,
        };
        let runtime_host = AgentSessionRuntimeHost::from_bootstrap(AgentSessionRuntimeBootstrap {
            cwd: Some(cwd),
            ..AgentSessionRuntimeBootstrap::default()
        });
        let options = SessionUiOptions {
            session_id: Some("seed-session".to_string()),
            ..SessionUiOptions::default()
        };
        let session_setup = SessionRuntimeSetup {
            conn,
            session_id: "seed-session".to_string(),
            provider: Arc::new(OpenAiProvider::new()),
            model,
            auth: None,
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".to_string(),
            headers: HashMap::new(),
            tool_registry: crate::tool_registry::ToolRegistry::default(),
            tool_selection: crate::tool_registry::ToolSelection::All,
            tool_ctx,
            system_prompt: String::new(),
            base_system_prompt: String::new(),
            thinking_level: "medium".to_string(),
            compaction_enabled: true,
            compaction_reserve_tokens: 8_000,
            compaction_keep_recent_tokens: 16_000,
            retry_enabled: true,
            retry_max_retries: 3,
            retry_base_delay_ms: 100,
            retry_max_delay_ms: 1_000,
            session_created: true,
            request_metrics_tracker: Arc::new(tokio::sync::Mutex::new(
                bb_monitor::RequestMetricsTracker::new(),
            )),
            request_metrics_log_path: None,
            sibling_conn: None,
            extension_commands: ExtensionCommandRegistry::default(),
            extension_bootstrap: ExtensionBootstrap::default(),
        };
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (_approval_tx, approval_rx) = mpsc::unbounded_channel();
        let controller = TuiController::new(
            runtime_host,
            options,
            session_setup,
            command_tx,
            approval_rx,
        );
        (controller, command_rx)
    }

    fn drain_commands(rx: &mut mpsc::UnboundedReceiver<TuiCommand>) -> Vec<TuiCommand> {
        let mut commands = Vec::new();
        while let Ok(command) = rx.try_recv() {
            commands.push(command);
        }
        commands
    }

    #[test]
    fn exact_model_match_with_multiple_auth_sources_prompts_for_auth_selection() {
        let _lock = env_lock().lock().unwrap();
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tempdir.path().join("Cargo.toml"),
            "[package]\nname='demo'\n",
        )
        .expect("cargo toml");
        let _home = EnvVarGuard::set_path("HOME", tempdir.path());
        let _openai = EnvVarGuard::set_value("OPENAI_API_KEY", "openai-test-key");
        crate::login::save_oauth_credentials(
            "openai-codex",
            &crate::oauth::OAuthCredentials {
                access: "oauth-access".to_string(),
                refresh: "refresh-token".to_string(),
                expires: i64::MAX,
                extra: serde_json::json!({"accountId": "acct_primary"}),
            },
        )
        .expect("save openai oauth");

        let (mut controller, mut command_rx) = build_test_controller(tempdir.path().to_path_buf());
        controller
            .handle_model_selection_command(Some("gpt-4o"))
            .expect("handle model selection");

        let commands = drain_commands(&mut command_rx);
        let menu = commands
            .into_iter()
            .find_map(|command| match command {
                TuiCommand::OpenSelectMenu {
                    menu_id,
                    title,
                    items,
                    ..
                } => Some((menu_id, title, items)),
                _ => None,
            })
            .expect("auth chooser menu");

        assert_eq!(menu.0, MODEL_AUTH_MENU_ID);
        assert_eq!(menu.1, "Select auth for OpenAI");
        assert_eq!(menu.2.len(), 2);
        assert_eq!(menu.2[0].label, "OAuth • acct_primary");
        assert!(
            menu.2[0].detail.as_deref().is_some_and(
                |detail| detail.contains("currently active") && detail.contains("saved ")
            )
        );
        assert_eq!(menu.2[1].label, "API key (env)");
        assert!(menu.2[1].value.starts_with("env:api-key"));
    }

    #[test]
    fn model_auth_menu_distinguishes_multiple_saved_api_keys() {
        let _lock = env_lock().lock().unwrap();
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tempdir.path().join("Cargo.toml"),
            "[package]\nname='demo'\n",
        )
        .expect("cargo toml");
        let _home = EnvVarGuard::set_path("HOME", tempdir.path());

        crate::login::save_api_key("openrouter", "key-1111".to_string()).expect("save first key");
        crate::login::save_api_key("openrouter", "key-2222".to_string()).expect("save second key");

        Settings {
            models: Some(vec![ModelOverride {
                id: "gpt-test".to_string(),
                name: Some("gpt-test".to_string()),
                provider: "openrouter".to_string(),
                api: Some("openai-completions".to_string()),
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                context_window: Some(128_000),
                max_tokens: Some(16_384),
                reasoning: Some(false),
                input: Some(vec!["text".to_string()]),
            }]),
            ..Settings::default()
        }
        .save_project(tempdir.path())
        .expect("save project settings");

        let (mut controller, mut command_rx) = build_test_controller(tempdir.path().to_path_buf());
        controller
            .handle_model_selection_command(Some("openrouter:gpt-test"))
            .expect("handle model selection");

        let commands = drain_commands(&mut command_rx);
        let menu = commands
            .into_iter()
            .find_map(|command| match command {
                TuiCommand::OpenSelectMenu {
                    menu_id,
                    title,
                    items,
                    ..
                } => Some((menu_id, title, items)),
                _ => None,
            })
            .expect("auth chooser menu");

        assert_eq!(menu.0, MODEL_AUTH_MENU_ID);
        assert_eq!(menu.1, "Select auth for OpenRouter");
        let key_2222 = menu
            .2
            .iter()
            .find(|item| item.label == "API key • ending in 2222")
            .expect("saved key 2222 option");
        let key_1111 = menu
            .2
            .iter()
            .find(|item| item.label == "API key • ending in 1111")
            .expect("saved key 1111 option");
        assert!(
            key_2222
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("profile "))
        );
        assert!(
            key_1111
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("profile "))
        );
    }

    #[test]
    fn ambiguous_exact_model_match_prompts_for_provider_selection() {
        let _lock = env_lock().lock().unwrap();
        let tempdir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tempdir.path().join("Cargo.toml"),
            "[package]\nname='demo'\n",
        )
        .expect("cargo toml");
        let _home = EnvVarGuard::set_path("HOME", tempdir.path());
        let _openai = EnvVarGuard::set_value("OPENAI_API_KEY", "openai-test-key");
        crate::login::save_api_key("openrouter", "openrouter-saved-key".to_string())
            .expect("save openrouter key");
        crate::login::save_oauth_credentials(
            "openai-codex",
            &crate::oauth::OAuthCredentials {
                access: "oauth-access".to_string(),
                refresh: "refresh-token".to_string(),
                expires: i64::MAX,
                extra: serde_json::json!({"accountId": "acct_primary"}),
            },
        )
        .expect("save openai oauth");

        Settings {
            models: Some(vec![ModelOverride {
                id: "gpt-4o".to_string(),
                name: Some("gpt-4o".to_string()),
                provider: "openrouter".to_string(),
                api: Some("openai-completions".to_string()),
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                context_window: Some(128_000),
                max_tokens: Some(16_384),
                reasoning: Some(false),
                input: Some(vec!["text".to_string()]),
            }]),
            ..Settings::default()
        }
        .save_project(tempdir.path())
        .expect("save project settings");

        let (mut controller, mut command_rx) = build_test_controller(tempdir.path().to_path_buf());
        assert_eq!(
            controller.matching_model_providers("gpt-4o"),
            vec!["openai".to_string(), "openrouter".to_string()]
        );
        controller
            .handle_model_selection_command(Some("gpt-4o"))
            .expect("handle model selection");

        let commands = drain_commands(&mut command_rx);
        let menu = commands
            .into_iter()
            .find_map(|command| match command {
                TuiCommand::OpenSelectMenu {
                    menu_id,
                    title,
                    items,
                    ..
                } => Some((menu_id, title, items)),
                _ => None,
            })
            .expect("provider chooser menu");

        assert_eq!(menu.0, MODEL_PROVIDER_MENU_ID);
        assert_eq!(menu.1, "Select provider for 'gpt-4o'");
        assert_eq!(
            controller.pending_model_provider_search.as_deref(),
            Some("gpt-4o")
        );
        assert_eq!(
            menu.2
                .iter()
                .map(|item| item.value.as_str())
                .collect::<Vec<_>>(),
            vec!["openai", "openrouter"]
        );
        assert_eq!(
            menu.2
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec!["OpenAI", "OpenRouter"]
        );
        assert!(
            menu.2[0]
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("active: OAuth • acct_primary • saved "))
        );
        assert!(
            menu.2[1]
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("active: API key • saved "))
        );
    }
}
