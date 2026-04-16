use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderAuthOptionSummary {
    pub method: ProviderAuthMethod,
    pub source: AuthSource,
    pub account_label: Option<String>,
    pub authority: Option<String>,
    pub configured_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub active: bool,
}

fn env_auth_methods_for_provider(provider: &str) -> Vec<ProviderAuthMethod> {
    match normalize_provider_for_model_selection(provider).as_str() {
        "anthropic" => std::env::var("ANTHROPIC_API_KEY")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
            .then_some(ProviderAuthMethod::ApiKey)
            .into_iter()
            .collect(),
        "openai" | "openai-codex" => std::env::var("OPENAI_API_KEY")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
            .then_some(ProviderAuthMethod::ApiKey)
            .into_iter()
            .collect(),
        "github-copilot" => ["GH_COPILOT_TOKEN", "GITHUB_COPILOT_TOKEN"]
            .iter()
            .any(|key| {
                std::env::var(key)
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
            })
            .then_some(ProviderAuthMethod::OAuth)
            .into_iter()
            .collect(),
        other => {
            let env_keys: &[&str] = match other {
                "google" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
                "groq" => &["GROQ_API_KEY"],
                "xai" => &["XAI_API_KEY"],
                "openrouter" => &["OPENROUTER_API_KEY"],
                _ => &[],
            };
            env_keys
                .iter()
                .any(|key| {
                    std::env::var(key)
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false)
                })
                .then_some(ProviderAuthMethod::ApiKey)
                .into_iter()
                .collect()
        }
    }
}

pub(crate) fn provider_auth_option_summaries(provider: &str) -> Vec<ProviderAuthOptionSummary> {
    let normalized = normalize_provider_for_model_selection(provider);
    let active_method = active_auth_method(&normalized);
    let mut options = stored_auth_profiles(&normalized)
        .into_iter()
        .map(|profile| ProviderAuthOptionSummary {
            method: profile.method,
            source: AuthSource::BbAuth,
            account_label: profile.account_label,
            authority: profile.authority,
            configured_at_ms: profile.configured_at_ms,
            updated_at_ms: profile.updated_at_ms,
            active: profile.active,
        })
        .collect::<Vec<_>>();

    let stored_methods = stored_auth_methods(&normalized);
    for method in env_auth_methods_for_provider(&normalized) {
        let active = stored_methods.is_empty() && active_method == Some(method);
        options.push(ProviderAuthOptionSummary {
            method,
            source: AuthSource::EnvVar,
            account_label: None,
            authority: None,
            configured_at_ms: None,
            updated_at_ms: None,
            active,
        });
    }

    options.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| match (left.source, right.source) {
                (AuthSource::BbAuth, AuthSource::EnvVar) => std::cmp::Ordering::Less,
                (AuthSource::EnvVar, AuthSource::BbAuth) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| {
                right
                    .configured_at_ms
                    .or(right.updated_at_ms)
                    .unwrap_or_default()
                    .cmp(
                        &left
                            .configured_at_ms
                            .or(left.updated_at_ms)
                            .unwrap_or_default(),
                    )
            })
            .then_with(|| left.method.label().cmp(right.method.label()))
    });
    options
}

fn auth_methods_for_provider(provider: &str) -> (bool, bool) {
    let stored = stored_auth_methods(provider);
    let env = env_auth_methods_for_provider(provider);
    let has_oauth =
        stored.contains(&ProviderAuthMethod::OAuth) || env.contains(&ProviderAuthMethod::OAuth);
    let has_api_key =
        stored.contains(&ProviderAuthMethod::ApiKey) || env.contains(&ProviderAuthMethod::ApiKey);
    (has_oauth, has_api_key)
}

fn format_configured_time(timestamp_ms: Option<i64>) -> Option<String> {
    let timestamp_ms = timestamp_ms?;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms)
        .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
}

fn render_auth_option_summary(summary: &ProviderAuthOptionSummary) -> String {
    let mut parts = Vec::new();
    match (summary.method, summary.source) {
        (ProviderAuthMethod::ApiKey, AuthSource::EnvVar) => parts.push("API key (env)".to_string()),
        (ProviderAuthMethod::ApiKey, AuthSource::BbAuth) => parts.push("API key".to_string()),
        (ProviderAuthMethod::OAuth, AuthSource::EnvVar) => parts.push("OAuth (env)".to_string()),
        (ProviderAuthMethod::OAuth, AuthSource::BbAuth) => parts.push("OAuth".to_string()),
    }
    if let Some(account_label) = &summary.account_label {
        parts.push(account_label.clone());
    }
    if let Some(authority) = &summary.authority {
        parts.push(authority.clone());
    }
    if let Some(saved_at) =
        format_configured_time(summary.configured_at_ms.or(summary.updated_at_ms))
    {
        parts.push(format!("saved {saved_at}"));
    }
    let detail = parts.join(" • ");
    if summary.active {
        format!("active: {detail}")
    } else {
        detail
    }
}

pub(crate) fn provider_model_selection_detail(provider: &str) -> String {
    let options = provider_auth_option_summaries(provider);
    if options.is_empty() {
        return "[not authenticated]".to_string();
    }
    options
        .iter()
        .map(render_auth_option_summary)
        .collect::<Vec<_>>()
        .join(" | ")
}

pub(crate) fn provider_auth_status_summary(provider: &str) -> String {
    let (has_oauth, has_api_key) = auth_methods_for_provider(provider);
    let active = active_auth_method(provider);
    let base = if has_oauth && has_api_key {
        "[OAuth + API key configured]".to_string()
    } else if has_oauth {
        "[OAuth configured]".to_string()
    } else if has_api_key {
        "[API key configured]".to_string()
    } else {
        "[not authenticated]".to_string()
    };

    match active {
        Some(method) if has_oauth && has_api_key => {
            format!("{base} • active: {}", method.label())
        }
        _ => base,
    }
}

pub(crate) fn add_cached_github_copilot_models(registry: &mut ModelRegistry) {
    for model_id in github_copilot_cached_models() {
        if registry.find("github-copilot", &model_id).is_none() {
            registry.add(Model {
                id: model_id.clone(),
                name: model_id.clone(),
                provider: "github-copilot".to_string(),
                api: bb_provider::registry::ApiType::OpenaiCompletions,
                context_window: 128_000,
                max_tokens: 16_384,
                reasoning: true,
                input: vec![bb_provider::registry::ModelInput::Text],
                base_url: Some(github_copilot_api_base_url()),
                cost: Default::default(),
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    BbAuth,
    EnvVar,
}

impl AuthSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            AuthSource::BbAuth => "bb auth.json",
            AuthSource::EnvVar => "environment",
        }
    }
}

pub fn auth_source(provider: &str) -> Option<AuthSource> {
    let store = load_auth();
    if !stored_auth_methods_for_store(&store, provider).is_empty() {
        return Some(AuthSource::BbAuth);
    }
    if !env_auth_methods_for_provider(provider).is_empty() {
        return Some(AuthSource::EnvVar);
    }
    None
}

pub fn provider_has_auth(provider: &str) -> bool {
    auth_source(provider).is_some()
}

pub fn authenticated_providers() -> Vec<String> {
    let mut out = Vec::new();
    for provider in known_providers().iter().map(|(name, _, _)| *name) {
        if !provider_has_auth(provider) {
            continue;
        }
        let normalized = normalize_provider_for_model_selection(provider);
        if !out.iter().any(|existing| existing == &normalized) {
            out.push(normalized);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        AuthSource, auth_source, provider_auth_option_summaries, provider_auth_status_summary,
        provider_model_selection_detail,
    };
    use crate::login::ProviderAuthMethod;
    use serde_json::json;
    use std::sync::Mutex;

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

    #[test]
    fn provider_model_selection_detail_lists_env_and_saved_profiles() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set_path("HOME", home.path());
        let _openai = EnvVarGuard::set_value("OPENAI_API_KEY", "openai-env-key");

        crate::login::save_oauth_credentials(
            "openai-codex",
            &crate::oauth::OAuthCredentials {
                access: "oauth-access".to_string(),
                refresh: "refresh-token".to_string(),
                expires: i64::MAX,
                extra: json!({"accountId": "acct_primary"}),
            },
        )
        .expect("save oauth credentials");

        let detail = provider_model_selection_detail("openai");
        assert!(detail.contains("active: OAuth • acct_primary • saved "));
        assert!(detail.contains("API key (env)"));
    }

    #[test]
    fn provider_auth_option_summaries_report_saved_profile_metadata() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set_path("HOME", home.path());

        crate::login::save_oauth_credentials(
            "github-copilot",
            &crate::oauth::OAuthCredentials {
                access: "oauth-access".to_string(),
                refresh: "refresh-token".to_string(),
                expires: i64::MAX,
                extra: json!({"domain": "github.example.com", "login": "octocat"}),
            },
        )
        .expect("save oauth credentials");

        let summaries = provider_auth_option_summaries("github-copilot");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].source, AuthSource::BbAuth);
        assert_eq!(summaries[0].method, ProviderAuthMethod::OAuth);
        assert_eq!(summaries[0].account_label.as_deref(), Some("octocat"));
        assert_eq!(
            summaries[0].authority.as_deref(),
            Some("github.example.com")
        );
        assert!(summaries[0].configured_at_ms.is_some());
        assert!(summaries[0].active);
    }

    #[test]
    fn provider_auth_status_summary_keeps_compact_method_summary() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set_path("HOME", home.path());
        let _openai = EnvVarGuard::set_value("OPENAI_API_KEY", "openai-env-key");

        crate::login::save_oauth_credentials(
            "openai-codex",
            &crate::oauth::OAuthCredentials {
                access: "oauth-access".to_string(),
                refresh: "refresh-token".to_string(),
                expires: i64::MAX,
                extra: json!({"accountId": "acct_primary"}),
            },
        )
        .expect("save oauth credentials");

        assert_eq!(
            provider_auth_status_summary("openai"),
            "[OAuth + API key configured] • active: OAuth"
        );
    }

    #[test]
    fn auth_source_prefers_saved_store_over_environment() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set_path("HOME", home.path());
        let _openai = EnvVarGuard::set_value("OPENAI_API_KEY", "openai-env-key");

        crate::login::save_api_key("openai", "saved-key".to_string()).expect("save api key");
        assert_eq!(auth_source("openai"), Some(AuthSource::BbAuth));
    }
}
