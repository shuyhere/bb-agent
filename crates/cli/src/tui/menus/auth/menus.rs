use super::dialogs::{tui_auth_display_name, tui_auth_status_detail};
use super::*;

fn provider_title(provider: &str) -> &str {
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
}

fn format_timestamp(timestamp_ms: Option<i64>) -> Option<String> {
    let timestamp_ms = timestamp_ms?;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
}

fn auth_option_label(option: &crate::login::ProviderAuthOptionSummary) -> String {
    match (option.method, option.source) {
        (crate::login::ProviderAuthMethod::ApiKey, crate::login::AuthSource::EnvVar) => {
            "API key (env)".to_string()
        }
        (crate::login::ProviderAuthMethod::ApiKey, crate::login::AuthSource::BbAuth) => {
            "Saved API key".to_string()
        }
        (crate::login::ProviderAuthMethod::OAuth, crate::login::AuthSource::EnvVar) => option
            .account_label
            .as_ref()
            .map(|label| format!("OAuth (env) • {label}"))
            .unwrap_or_else(|| "OAuth (env)".to_string()),
        (crate::login::ProviderAuthMethod::OAuth, crate::login::AuthSource::BbAuth) => option
            .account_label
            .as_ref()
            .map(|label| format!("OAuth • {label}"))
            .unwrap_or_else(|| "OAuth".to_string()),
    }
}

fn auth_option_detail(option: &crate::login::ProviderAuthOptionSummary) -> Option<String> {
    let mut parts = Vec::new();
    if option.active {
        parts.push("currently active".to_string());
    }
    if let Some(authority) = &option.authority {
        parts.push(authority.clone());
    }
    if let Some(saved_at) = format_timestamp(option.configured_at_ms.or(option.updated_at_ms)) {
        parts.push(format!("saved {saved_at}"));
    }
    (!parts.is_empty()).then_some(parts.join(" • "))
}

fn auth_option_value(option: &crate::login::ProviderAuthOptionSummary) -> String {
    option
        .profile_id
        .as_ref()
        .map(|profile_id| format!("profile:{profile_id}"))
        .unwrap_or_else(|| format!("env:{}", option.method.footer_label()))
}

fn auth_method_detail(
    provider: &str,
    method: crate::login::ProviderAuthMethod,
    base: &str,
) -> String {
    let options = crate::login::provider_auth_option_summaries(provider)
        .into_iter()
        .filter(|option| option.method == method)
        .collect::<Vec<_>>();
    if options.is_empty() {
        return base.to_string();
    }

    let mut detail = format!(
        "{base} • {} option{}",
        options.len(),
        if options.len() == 1 { "" } else { "s" }
    );
    if let Some(active) = options.iter().find(|option| option.active) {
        detail.push_str(" • active: ");
        detail.push_str(&auth_option_label(active));
        if let Some(authority) = &active.authority {
            detail.push_str(" • ");
            detail.push_str(authority);
        }
    }
    detail
}

impl TuiController {
    pub(crate) fn maybe_open_login_auth_option_menu(
        &mut self,
        provider: &str,
        method: crate::login::ProviderAuthMethod,
    ) -> bool {
        let options = crate::login::provider_auth_option_summaries(provider)
            .into_iter()
            .filter(|option| option.method == method)
            .collect::<Vec<_>>();
        if options.is_empty() {
            return false;
        }

        self.pending_login_auth_selection =
            Some(crate::tui::controller::PendingLoginAuthSelection {
                provider: provider.to_string(),
                method,
            });

        let mut items = options
            .into_iter()
            .map(|option| SelectItem {
                label: auth_option_label(&option),
                detail: auth_option_detail(&option),
                value: auth_option_value(&option),
            })
            .collect::<Vec<_>>();

        match (provider, method) {
            ("github-copilot", crate::login::ProviderAuthMethod::OAuth) => {
                items.push(SelectItem {
                    label: "Sign in with GitHub.com".to_string(),
                    detail: Some(
                        "Start a new Copilot OAuth/device login for github.com".to_string(),
                    ),
                    value: "action:copilot-github".to_string(),
                });
                items.push(SelectItem {
                    label: "Sign in with GitHub Enterprise Server".to_string(),
                    detail: Some(
                        "Choose a GitHub Enterprise Server host and start a new Copilot login"
                            .to_string(),
                    ),
                    value: "action:copilot-enterprise".to_string(),
                });
            }
            (_, crate::login::ProviderAuthMethod::OAuth) => {
                items.push(SelectItem {
                    label: "Sign in another account".to_string(),
                    detail: Some("Store another saved OAuth profile".to_string()),
                    value: "action:login-new".to_string(),
                });
            }
            (_, crate::login::ProviderAuthMethod::ApiKey) => {
                items.push(SelectItem {
                    label: "Paste a new API key".to_string(),
                    detail: Some("Save or replace the API key stored in auth.json".to_string()),
                    value: "action:login-new".to_string(),
                });
            }
        }

        let method_label = match method {
            crate::login::ProviderAuthMethod::OAuth => "OAuth",
            crate::login::ProviderAuthMethod::ApiKey => "API key",
        };
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: LOGIN_AUTH_OPTION_MENU_ID.to_string(),
            title: format!("Use {} {}", provider_title(provider), method_label),
            items,
            selected_value: None,
        });
        true
    }

    pub(crate) fn open_login_provider_menu(&mut self) {
        self.send_command(TuiCommand::OpenSelectMenu {
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
                        label: provider_title(provider).to_string(),
                        detail: Some(format!("{methods} • {}", tui_auth_status_detail(provider))),
                        value: (*provider).to_string(),
                    }
                })
                .collect(),
            selected_value: None,
        });
    }

    pub(crate) fn open_login_method_menu(&mut self, provider: &str) {
        let mut items = Vec::new();
        match provider {
            "anthropic" => {
                items.push(SelectItem {
                    label: "Claude Pro/Max".to_string(),
                    detail: Some(auth_method_detail(
                        "anthropic",
                        crate::login::ProviderAuthMethod::OAuth,
                        "OAuth subscription login",
                    )),
                    value: "oauth:anthropic".to_string(),
                });
                items.push(SelectItem {
                    label: "Anthropic API key".to_string(),
                    detail: Some(auth_method_detail(
                        "anthropic",
                        crate::login::ProviderAuthMethod::ApiKey,
                        "Use ANTHROPIC_API_KEY or paste a key",
                    )),
                    value: "api_key:anthropic".to_string(),
                });
            }
            "openai" => {
                items.push(SelectItem {
                    label: "ChatGPT Plus/Pro (Codex)".to_string(),
                    detail: Some(auth_method_detail(
                        "openai",
                        crate::login::ProviderAuthMethod::OAuth,
                        "OAuth subscription login",
                    )),
                    value: "oauth:openai-codex".to_string(),
                });
                items.push(SelectItem {
                    label: "OpenAI API key".to_string(),
                    detail: Some(auth_method_detail(
                        "openai",
                        crate::login::ProviderAuthMethod::ApiKey,
                        "Use OPENAI_API_KEY or paste a key",
                    )),
                    value: "api_key:openai".to_string(),
                });
            }
            "github-copilot" => {
                items.push(SelectItem {
                    label: "Use existing Copilot login".to_string(),
                    detail: Some(auth_method_detail(
                        "github-copilot",
                        crate::login::ProviderAuthMethod::OAuth,
                        "Switch between saved or env-backed Copilot auth",
                    )),
                    value: "oauth:github-copilot".to_string(),
                });
                items.push(SelectItem {
                    label: "Sign in with GitHub.com".to_string(),
                    detail: Some("Start a new github.com Copilot login".to_string()),
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
                    detail: Some(auth_method_detail(
                        "google",
                        crate::login::ProviderAuthMethod::ApiKey,
                        "Use GOOGLE_API_KEY / GEMINI_API_KEY or paste a key",
                    )),
                    value: "api_key:google".to_string(),
                });
            }
            "groq" => {
                items.push(SelectItem {
                    label: "Groq API key".to_string(),
                    detail: Some(auth_method_detail(
                        "groq",
                        crate::login::ProviderAuthMethod::ApiKey,
                        "Use GROQ_API_KEY or paste a key",
                    )),
                    value: "api_key:groq".to_string(),
                });
            }
            "xai" => {
                items.push(SelectItem {
                    label: "xAI API key".to_string(),
                    detail: Some(auth_method_detail(
                        "xai",
                        crate::login::ProviderAuthMethod::ApiKey,
                        "Use XAI_API_KEY or paste a key",
                    )),
                    value: "api_key:xai".to_string(),
                });
            }
            "openrouter" => {
                items.push(SelectItem {
                    label: "OpenRouter API key".to_string(),
                    detail: Some(auth_method_detail(
                        "openrouter",
                        crate::login::ProviderAuthMethod::ApiKey,
                        "Use OPENROUTER_API_KEY or paste a key",
                    )),
                    value: "api_key:openrouter".to_string(),
                });
            }
            _ => {}
        }

        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: LOGIN_METHOD_MENU_ID.to_string(),
            title: format!("Sign in method: {}", provider_title(provider)),
            items,
            selected_value: None,
        });
    }

    pub(crate) fn open_logout_provider_menu(&mut self) {
        let providers = crate::login::configured_providers();
        if providers.is_empty() {
            self.send_command(TuiCommand::SetStatusLine(
                "No logged-in providers".to_string(),
            ));
            return;
        }
        self.send_command(TuiCommand::OpenSelectMenu {
            menu_id: LOGOUT_PROVIDER_MENU_ID.to_string(),
            title: "Logout provider".to_string(),
            items: providers
                .into_iter()
                .map(|provider| SelectItem {
                    label: tui_auth_display_name(&provider),
                    detail: Some(tui_auth_status_detail(&provider)),
                    value: provider,
                })
                .collect(),
            selected_value: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use bb_core::agent_session_runtime::{AgentSessionRuntimeBootstrap, AgentSessionRuntimeHost};
    use bb_provider::openai::OpenAiProvider;
    use bb_provider::registry::{ApiType, CostConfig, Model, ModelInput};
    use bb_session::store;
    use bb_tools::{ExecutionPolicy, ToolContext, ToolExecutionMode};
    use bb_tui::tui::TuiCommand;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;

    use crate::extensions::{ExtensionBootstrap, ExtensionCommandRegistry};
    use crate::session_bootstrap::{SessionRuntimeSetup, SessionUiOptions};
    use crate::tui::controller::TuiController;
    use crate::tui::{LOGIN_AUTH_OPTION_MENU_ID, LOGIN_METHOD_MENU_ID};

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

    fn build_test_controller(
        cwd: std::path::PathBuf,
    ) -> (TuiController, mpsc::UnboundedReceiver<TuiCommand>) {
        let conn = store::open_memory().expect("memory db");
        let model = Model {
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
        };
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
        let options = SessionUiOptions::default();
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
    fn openai_login_method_detail_reports_multiple_saved_options() {
        let _lock = env_lock().lock().unwrap();
        let tempdir = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set_path("HOME", tempdir.path());
        let _openai = EnvVarGuard::set_value("OPENAI_API_KEY", "openai-env-key");
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
        controller.open_login_method_menu("openai");
        let commands = drain_commands(&mut command_rx);
        let menu = commands
            .into_iter()
            .find_map(|command| match command {
                TuiCommand::OpenSelectMenu { menu_id, items, .. } => Some((menu_id, items)),
                _ => None,
            })
            .expect("login method menu");

        assert_eq!(menu.0, LOGIN_METHOD_MENU_ID);
        let oauth = menu
            .1
            .iter()
            .find(|item| item.value == "oauth:openai-codex")
            .expect("oauth item");
        let api_key = menu
            .1
            .iter()
            .find(|item| item.value == "api_key:openai")
            .expect("api key item");
        assert!(
            oauth
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("1 option"))
        );
        assert!(
            api_key
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("1 option"))
        );
    }

    #[test]
    fn openai_login_method_opens_auth_option_menu_when_options_exist() {
        let _lock = env_lock().lock().unwrap();
        let tempdir = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set_path("HOME", tempdir.path());
        let _openai = EnvVarGuard::set_value("OPENAI_API_KEY", "openai-env-key");
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
        assert!(
            controller.maybe_open_login_auth_option_menu(
                "openai",
                crate::login::ProviderAuthMethod::OAuth,
            )
        );

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
            .expect("auth option menu");

        assert_eq!(menu.0, LOGIN_AUTH_OPTION_MENU_ID);
        assert_eq!(menu.1, "Use OpenAI OAuth");
        assert!(
            menu.2
                .iter()
                .any(|item| item.label == "OAuth • acct_primary")
        );
        assert!(
            menu.2
                .iter()
                .any(|item| item.label == "Sign in another account")
        );
    }
}
