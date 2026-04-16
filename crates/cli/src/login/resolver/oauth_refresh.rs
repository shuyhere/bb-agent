use super::store::{save_auth, save_oauth_state};
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
    let mut store = load_auth();
    store.providers.insert(
        provider.to_string(),
        AuthEntry::OAuth {
            access: creds.access.clone(),
            refresh: creds.refresh.clone(),
            expires: creds.expires,
            extra: creds.extra.clone(),
        },
    );
    store.last_provider = Some(normalize_provider_for_model_selection(provider));
    save_auth(&store)
}

pub fn resolve_provider_auth(provider: &str) -> Option<ResolvedProviderAuth> {
    if provider == "github-copilot" {
        return resolve_github_copilot_auth();
    }

    let store_keys: &[&str] = match provider {
        "openai" => &["openai", "openai-codex"],
        "openai-codex" => &["openai-codex", "openai"],
        _ => &[provider],
    };

    let store = load_auth();
    for &key_name in store_keys {
        let Some(entry) = store.providers.get(key_name) else {
            continue;
        };
        match entry {
            AuthEntry::ApiKey { key } if !key.trim().is_empty() => {
                return Some(ResolvedProviderAuth {
                    source: AuthSource::BbAuth,
                    credential_provider: key_name.to_string(),
                    method: ProviderAuthMethod::ApiKey,
                    credential: key.clone(),
                    account_id: None,
                    account_label: None,
                    authority: None,
                });
            }
            AuthEntry::OAuth {
                access,
                refresh,
                expires,
                extra,
            } if !access.trim().is_empty() => {
                let account_id = extra
                    .get("accountId")
                    .and_then(|value| value.as_str())
                    .map(ToString::to_string);
                let now_ms = chrono::Utc::now().timestamp_millis();
                let credential = if *expires > now_ms + 60_000 {
                    access.clone()
                } else if !refresh.is_empty() {
                    try_refresh_sync(key_name, refresh).unwrap_or_else(|| access.clone())
                } else {
                    access.clone()
                };
                return Some(ResolvedProviderAuth {
                    source: AuthSource::BbAuth,
                    credential_provider: key_name.to_string(),
                    method: ProviderAuthMethod::OAuth,
                    credential,
                    account_id: account_id.clone(),
                    account_label: account_id,
                    authority: None,
                });
            }
            _ => {}
        }
    }

    let env_keys: &[&str] = match provider {
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "openai" | "openai-codex" => &["OPENAI_API_KEY"],
        "google" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        "groq" => &["GROQ_API_KEY"],
        "xai" => &["XAI_API_KEY"],
        "openrouter" => &["OPENROUTER_API_KEY"],
        _ => &[],
    };

    for key in env_keys {
        if let Ok(val) = std::env::var(key)
            && !val.is_empty()
        {
            return Some(ResolvedProviderAuth {
                source: AuthSource::EnvVar,
                credential_provider: provider.to_string(),
                method: ProviderAuthMethod::ApiKey,
                credential: val,
                account_id: None,
                account_label: None,
                authority: None,
            });
        }
    }

    None
}

fn resolve_github_copilot_auth() -> Option<ResolvedProviderAuth> {
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

    let store = load_auth();
    let entry = store.providers.get("github-copilot")?.clone();
    let AuthEntry::OAuth {
        access,
        refresh,
        expires,
        extra,
    } = entry
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
    use super::{ResolvedProviderAuth, resolve_provider_auth};
    use crate::login::ProviderAuthMethod;
    use crate::login::store::{AuthEntry, AuthStore, save_auth};
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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
            providers,
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
}
