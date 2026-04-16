use super::store::save_oauth_state;
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedProviderAuth {
    pub source: AuthSource,
    pub credential_provider: String,
    pub method: ProviderAuthMethod,
    pub credential: String,
    pub account_id: Option<String>,
    pub account_label: Option<String>,
    pub authority: Option<String>,
}

impl ResolvedProviderAuth {
    pub(crate) fn footer_badge(&self, provider: &str) -> String {
        let method = self.method.footer_label();
        match self.source {
            AuthSource::BbAuth => format!("{provider}/{method}"),
            AuthSource::EnvVar => format!("{provider}/{method}(env)"),
        }
    }
}

pub fn save_oauth_credentials(provider: &str, creds: &OAuthCredentials) -> Result<()> {
    save_oauth_state(
        provider,
        creds.access.clone(),
        creds.refresh.clone(),
        creds.expires,
        creds.extra.clone(),
    )
}

fn resolve_stored_profile_auth(
    provider: &str,
    profile: &AuthProfile,
) -> Option<ResolvedProviderAuth> {
    let normalized = normalize_provider_for_model_selection(provider);
    match &profile.entry {
        AuthEntry::ApiKey { key } => Some(ResolvedProviderAuth {
            source: AuthSource::BbAuth,
            credential_provider: provider_storage_key(&normalized, profile.method),
            method: profile.method,
            credential: key.clone(),
            account_id: None,
            account_label: None,
            authority: None,
        }),
        AuthEntry::OAuth {
            access,
            refresh,
            expires,
            extra,
        } => {
            let account_id = extra
                .get("accountId")
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
            let authority = extra
                .get("domain")
                .and_then(|value| value.as_str())
                .map(ToString::to_string);
            let account_label = account_id.clone().or_else(|| {
                extra
                    .get("login")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string)
            });
            let now_ms = chrono::Utc::now().timestamp_millis();
            let credential_provider = provider_storage_key(&normalized, profile.method);
            let credential = if *expires > now_ms + 60_000 {
                access.clone()
            } else if !refresh.is_empty() {
                try_refresh_sync(&credential_provider, refresh.as_str())
                    .unwrap_or_else(|| access.clone())
            } else {
                access.clone()
            };
            Some(ResolvedProviderAuth {
                source: AuthSource::BbAuth,
                credential_provider,
                method: profile.method,
                credential,
                account_id,
                account_label,
                authority,
            })
        }
        AuthEntry::ProviderConfig { .. } => None,
    }
}

fn resolve_env_provider_auth(
    provider: &str,
    method: ProviderAuthMethod,
) -> Option<ResolvedProviderAuth> {
    let normalized = normalize_provider_for_model_selection(provider);
    match (normalized.as_str(), method) {
        ("anthropic", ProviderAuthMethod::ApiKey) => std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|val| !val.is_empty())
            .map(|val| ResolvedProviderAuth {
                source: AuthSource::EnvVar,
                credential_provider: normalized.clone(),
                method,
                credential: val,
                account_id: None,
                account_label: None,
                authority: None,
            }),
        ("openai" | "openai-codex", ProviderAuthMethod::ApiKey) => std::env::var("OPENAI_API_KEY")
            .ok()
            .filter(|val| !val.is_empty())
            .map(|val| ResolvedProviderAuth {
                source: AuthSource::EnvVar,
                credential_provider: normalized.clone(),
                method,
                credential: val,
                account_id: None,
                account_label: None,
                authority: None,
            }),
        ("google", ProviderAuthMethod::ApiKey) => ["GOOGLE_API_KEY", "GEMINI_API_KEY"]
            .into_iter()
            .find_map(|key| std::env::var(key).ok().filter(|val| !val.is_empty()))
            .map(|val| ResolvedProviderAuth {
                source: AuthSource::EnvVar,
                credential_provider: normalized.clone(),
                method,
                credential: val,
                account_id: None,
                account_label: None,
                authority: None,
            }),
        ("groq", ProviderAuthMethod::ApiKey) => std::env::var("GROQ_API_KEY")
            .ok()
            .filter(|val| !val.is_empty())
            .map(|val| ResolvedProviderAuth {
                source: AuthSource::EnvVar,
                credential_provider: normalized.clone(),
                method,
                credential: val,
                account_id: None,
                account_label: None,
                authority: None,
            }),
        ("xai", ProviderAuthMethod::ApiKey) => std::env::var("XAI_API_KEY")
            .ok()
            .filter(|val| !val.is_empty())
            .map(|val| ResolvedProviderAuth {
                source: AuthSource::EnvVar,
                credential_provider: normalized.clone(),
                method,
                credential: val,
                account_id: None,
                account_label: None,
                authority: None,
            }),
        ("openrouter", ProviderAuthMethod::ApiKey) => std::env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|val| !val.is_empty())
            .map(|val| ResolvedProviderAuth {
                source: AuthSource::EnvVar,
                credential_provider: normalized.clone(),
                method,
                credential: val,
                account_id: None,
                account_label: None,
                authority: None,
            }),
        ("github-copilot", ProviderAuthMethod::OAuth) => resolve_github_copilot_env_auth(),
        _ => None,
    }
}

pub fn resolve_provider_auth(provider: &str) -> Option<ResolvedProviderAuth> {
    let normalized = normalize_provider_for_model_selection(provider);
    if normalized == "github-copilot" {
        return resolve_github_copilot_auth();
    }

    let store = load_auth();
    let preferred_methods = match active_auth_method(&normalized) {
        Some(active) => match active {
            ProviderAuthMethod::OAuth => [ProviderAuthMethod::OAuth, ProviderAuthMethod::ApiKey],
            ProviderAuthMethod::ApiKey => [ProviderAuthMethod::ApiKey, ProviderAuthMethod::OAuth],
        },
        None => [ProviderAuthMethod::ApiKey, ProviderAuthMethod::OAuth],
    };

    for method in preferred_methods {
        if let Some(profile) = stored_auth_profile_for_method(&store, &normalized, method)
            && let Some(auth) = resolve_stored_profile_auth(&normalized, profile)
        {
            return Some(auth);
        }
    }

    [ProviderAuthMethod::ApiKey, ProviderAuthMethod::OAuth]
        .into_iter()
        .find_map(|method| resolve_env_provider_auth(&normalized, method))
}

pub(crate) fn resolve_provider_auth_choice(
    provider: &str,
    choice: &str,
) -> Option<ResolvedProviderAuth> {
    let normalized = normalize_provider_for_model_selection(provider);
    if let Some(profile_id) = choice.strip_prefix("profile:") {
        let store = load_auth();
        let profile = stored_auth_profile_by_id(&store, &normalized, profile_id)?;
        if normalized == "github-copilot" {
            return resolve_github_copilot_profile_auth(profile);
        }
        return resolve_stored_profile_auth(&normalized, profile);
    }
    if let Some(method) = choice
        .strip_prefix("env:")
        .and_then(parse_auth_method_choice)
    {
        return resolve_env_provider_auth(&normalized, method);
    }
    None
}

fn parse_auth_method_choice(value: &str) -> Option<ProviderAuthMethod> {
    match value {
        "oauth" => Some(ProviderAuthMethod::OAuth),
        "api-key" => Some(ProviderAuthMethod::ApiKey),
        _ => None,
    }
}

fn resolve_github_copilot_env_auth() -> Option<ResolvedProviderAuth> {
    for key in ["GH_COPILOT_TOKEN", "GITHUB_COPILOT_TOKEN"] {
        if let Ok(val) = std::env::var(key)
            && !val.trim().is_empty()
        {
            return Some(ResolvedProviderAuth {
                source: AuthSource::EnvVar,
                credential_provider: "github-copilot".to_string(),
                method: ProviderAuthMethod::OAuth,
                credential: val,
                account_id: None,
                account_label: None,
                authority: None,
            });
        }
    }
    None
}

fn resolve_github_copilot_profile_auth(profile: &AuthProfile) -> Option<ResolvedProviderAuth> {
    let AuthEntry::OAuth {
        access,
        refresh,
        expires,
        extra,
    } = profile.entry.clone()
    else {
        return None;
    };

    let authority = extra
        .get("domain")
        .and_then(|value| value.as_str())
        .map(ToString::to_string)
        .or_else(github_copilot_domain)
        .unwrap_or_else(|| "github.com".to_string());
    let account_label = extra
        .get("login")
        .and_then(|value| value.as_str())
        .map(ToString::to_string);
    let now_ms = chrono::Utc::now().timestamp_millis();

    if let Some(token) = extra.get("copilot_token").and_then(|value| value.as_str())
        && let Some(expires_at) = extra
            .get("copilot_expires_at")
            .and_then(|value| value.as_i64())
        && expires_at > now_ms + 300_000
        && !token.trim().is_empty()
    {
        return Some(ResolvedProviderAuth {
            source: AuthSource::BbAuth,
            credential_provider: "github-copilot".to_string(),
            method: ProviderAuthMethod::OAuth,
            credential: token.to_string(),
            account_id: None,
            account_label,
            authority: Some(authority),
        });
    }

    if expires <= now_ms + 60_000
        && !refresh.trim().is_empty()
        && let Some(token) = try_refresh_sync("github-copilot", &refresh)
    {
        return Some(ResolvedProviderAuth {
            source: AuthSource::BbAuth,
            credential_provider: "github-copilot".to_string(),
            method: ProviderAuthMethod::OAuth,
            credential: token,
            account_id: None,
            account_label,
            authority: Some(authority),
        });
    }

    if access.trim().is_empty() {
        return None;
    }

    let refreshed = refresh_github_copilot_runtime_sync(&authority, &access)?;
    let mut extra = extra;
    merge_github_copilot_runtime_extra(&mut extra, &authority, &refreshed);
    let _ = save_oauth_state("github-copilot", access, refresh, expires, extra);
    Some(ResolvedProviderAuth {
        source: AuthSource::BbAuth,
        credential_provider: "github-copilot".to_string(),
        method: ProviderAuthMethod::OAuth,
        credential: refreshed.copilot_token,
        account_id: None,
        account_label: refreshed.login.clone(),
        authority: Some(authority),
    })
}

fn resolve_github_copilot_auth() -> Option<ResolvedProviderAuth> {
    resolve_github_copilot_env_auth().or_else(|| {
        let store = load_auth();
        let profile =
            stored_auth_profile_for_method(&store, "github-copilot", ProviderAuthMethod::OAuth)?;
        resolve_github_copilot_profile_auth(profile)
    })
}

fn merge_github_copilot_runtime_extra(
    extra: &mut serde_json::Value,
    authority: &str,
    runtime: &crate::oauth::github_copilot::CopilotRuntimeSession,
) {
    let mut map = extra.as_object().cloned().unwrap_or_default();
    map.insert(
        "domain".to_string(),
        serde_json::Value::String(authority.to_string()),
    );
    map.insert(
        "login".to_string(),
        runtime
            .login
            .as_ref()
            .map(|value| serde_json::Value::String(value.clone()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "copilot_token".to_string(),
        serde_json::Value::String(runtime.copilot_token.clone()),
    );
    map.insert(
        "copilot_expires_at".to_string(),
        serde_json::Value::Number(runtime.copilot_expires_at_ms.into()),
    );
    map.insert(
        "copilot_api_base_url".to_string(),
        serde_json::Value::String(runtime.api_base_url.clone()),
    );
    map.insert(
        "copilot_models".to_string(),
        serde_json::Value::Array(
            runtime
                .models
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    map.insert(
        "organization_list".to_string(),
        serde_json::Value::Array(
            runtime
                .organization_list
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    map.insert(
        "enterprise_list".to_string(),
        serde_json::Value::Array(
            runtime
                .enterprise_list
                .iter()
                .cloned()
                .map(serde_json::Value::String)
                .collect(),
        ),
    );
    map.insert(
        "sku".to_string(),
        runtime
            .sku
            .as_ref()
            .map(|value| serde_json::Value::String(value.clone()))
            .unwrap_or(serde_json::Value::Null),
    );
    map.insert(
        "copilot_endpoints".to_string(),
        serde_json::to_value(runtime.raw_endpoints.clone()).unwrap_or(serde_json::Value::Null),
    );
    *extra = serde_json::Value::Object(map);
}

fn refresh_github_copilot_runtime_sync(
    authority: &str,
    github_access_token: &str,
) -> Option<crate::oauth::github_copilot::CopilotRuntimeSession> {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(_handle) => {
            let authority = authority.to_string();
            let github_access_token = github_access_token.to_string();
            return std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().ok()?;
                rt.block_on(
                    crate::oauth::github_copilot::exchange_github_token_for_copilot_session(
                        &authority,
                        &github_access_token,
                    ),
                )
                .ok()
            })
            .join()
            .ok()
            .flatten();
        }
        Err(_) => tokio::runtime::Runtime::new().ok()?,
    };
    rt.block_on(
        crate::oauth::github_copilot::exchange_github_token_for_copilot_session(
            authority,
            github_access_token,
        ),
    )
    .ok()
}

fn try_refresh_sync(provider: &str, refresh_token: &str) -> Option<String> {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(_handle) => {
            let provider = provider.to_string();
            let refresh_token = refresh_token.to_string();
            let result = std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().ok()?;
                rt.block_on(do_refresh(&provider, &refresh_token))
            })
            .join()
            .ok()
            .flatten();
            return result;
        }
        Err(_) => tokio::runtime::Runtime::new().ok()?,
    };
    rt.block_on(do_refresh(provider, refresh_token))
}

async fn do_refresh(provider: &str, refresh_token: &str) -> Option<String> {
    use crate::oauth;

    let provider = match provider {
        "anthropic-oauth" => "anthropic",
        other => other,
    };

    let creds = match provider {
        "anthropic" => oauth::anthropic::refresh_anthropic_token(refresh_token)
            .await
            .ok()?,
        "openai" | "openai-codex" => oauth::openai_codex::refresh_openai_codex_token(refresh_token)
            .await
            .ok()?,
        "github-copilot" => oauth::github_copilot::refresh_github_copilot_token(
            refresh_token,
            &github_copilot_domain().unwrap_or_else(|| "github.com".to_string()),
        )
        .await
        .ok()?,
        _ => return None,
    };

    let _ = save_oauth_credentials(provider, &creds);
    if provider == "github-copilot" {
        creds
            .extra
            .get("copilot_token")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
            .or(Some(creds.access))
    } else {
        Some(creds.access)
    }
}

#[cfg(test)]
mod tests {
    use super::{ResolvedProviderAuth, resolve_provider_auth, resolve_provider_auth_choice};
    use crate::login::ProviderAuthMethod;
    use crate::login::store::{AuthEntry, AuthStore, save_auth};
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn env_lock() -> &'static Mutex<()> {
        crate::login::auth_test_env_lock()
    }

    struct EnvVarGuard {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
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
    fn resolves_openai_provider_to_codex_oauth_when_only_oauth_is_configured() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set("HOME", home.path());

        let mut providers = HashMap::new();
        providers.insert(
            "openai-codex".to_string(),
            AuthEntry::OAuth {
                access: "oauth-access".to_string(),
                refresh: String::new(),
                expires: i64::MAX,
                extra: serde_json::json!({"accountId": "acct_test123"}),
            },
        );
        save_auth(&AuthStore {
            last_provider: Some("openai".to_string()),
            active_auth_methods: HashMap::new(),
            providers,
            ..AuthStore::default()
        })
        .expect("save auth");

        let resolved = resolve_provider_auth("openai").expect("resolved auth");
        assert_eq!(
            resolved,
            ResolvedProviderAuth {
                source: crate::login::resolver::AuthSource::BbAuth,
                credential_provider: "openai-codex".to_string(),
                method: ProviderAuthMethod::OAuth,
                credential: "oauth-access".to_string(),
                account_id: Some("acct_test123".to_string()),
                account_label: Some("acct_test123".to_string()),
                authority: None,
            }
        );
    }

    #[test]
    fn resolve_provider_auth_choice_can_pick_environment_api_key_over_saved_oauth() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set("HOME", home.path());
        unsafe { std::env::set_var("OPENAI_API_KEY", "env-openai-key") };

        let mut providers = HashMap::new();
        providers.insert(
            "openai-codex".to_string(),
            AuthEntry::OAuth {
                access: "oauth-access".to_string(),
                refresh: String::new(),
                expires: i64::MAX,
                extra: serde_json::json!({"accountId": "acct_test123"}),
            },
        );
        save_auth(&AuthStore {
            last_provider: Some("openai".to_string()),
            active_auth_methods: HashMap::new(),
            providers,
            ..AuthStore::default()
        })
        .expect("save auth");

        let resolved =
            resolve_provider_auth_choice("openai", "env:api-key").expect("resolved auth");
        assert_eq!(resolved.source, crate::login::resolver::AuthSource::EnvVar);
        assert_eq!(resolved.method, ProviderAuthMethod::ApiKey);
        assert_eq!(resolved.credential, "env-openai-key");
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    fn resolves_anthropic_to_active_api_key_when_both_methods_are_saved() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set("HOME", home.path());

        let mut providers = HashMap::new();
        providers.insert(
            "anthropic".to_string(),
            AuthEntry::ApiKey {
                key: "api-key-secret".to_string(),
            },
        );
        providers.insert(
            "anthropic-oauth".to_string(),
            AuthEntry::OAuth {
                access: "oauth-access".to_string(),
                refresh: String::new(),
                expires: i64::MAX,
                extra: serde_json::json!({}),
            },
        );
        save_auth(&AuthStore {
            last_provider: Some("anthropic".to_string()),
            active_auth_methods: HashMap::from([(
                "anthropic".to_string(),
                ProviderAuthMethod::ApiKey,
            )]),
            providers,
            ..AuthStore::default()
        })
        .expect("save auth");

        let resolved = resolve_provider_auth("anthropic").expect("resolved auth");
        assert_eq!(resolved.method, ProviderAuthMethod::ApiKey);
        assert_eq!(resolved.credential, "api-key-secret");
        assert_eq!(resolved.credential_provider, "anthropic");
    }
}
