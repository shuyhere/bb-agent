use super::*;

#[derive(Debug, Serialize, Deserialize, Default)]
pub(super) struct AuthStore {
    #[serde(default)]
    pub(super) last_provider: Option<String>,
    #[serde(flatten)]
    pub(super) providers: HashMap<String, AuthEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(super) enum AuthEntry {
    #[serde(rename = "api_key")]
    ApiKey { key: String },
    #[serde(rename = "oauth")]
    OAuth {
        access: String,
        refresh: String,
        expires: i64,
        #[serde(flatten)]
        extra: serde_json::Value,
    },
    #[serde(rename = "provider_config")]
    ProviderConfig { domain: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GithubCopilotStatus {
    pub authority: Option<String>,
    pub login: Option<String>,
    pub api_base_url: Option<String>,
    pub cached_models: Vec<String>,
    pub github_access_expires_at: Option<i64>,
    pub github_refresh_expires_at: Option<i64>,
    pub copilot_expires_at: Option<i64>,
    pub has_oauth: bool,
}

pub fn remove_auth(provider: &str) -> Result<bool> {
    let mut store = load_auth();
    let removed = store.providers.remove(provider).is_some();
    if removed {
        if store.last_provider.as_deref() == Some(provider)
            || store.last_provider.as_deref()
                == Some(normalize_provider_for_model_selection(provider).as_str())
        {
            store.last_provider = None;
        }
        save_auth(&store)?;
    }
    Ok(removed)
}

pub fn auth_path() -> PathBuf {
    config::global_dir().join("auth.json")
}

pub(super) fn load_auth() -> AuthStore {
    let path = auth_path();
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => AuthStore::default(),
        }
    } else {
        AuthStore::default()
    }
}

pub(super) fn save_auth(store: &AuthStore) -> Result<()> {
    let path = auth_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(store)?;
    std::fs::write(&path, &content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

pub(crate) fn save_api_key(provider: &str, key: String) -> Result<()> {
    let mut store = load_auth();
    store
        .providers
        .insert(provider.to_string(), AuthEntry::ApiKey { key });
    store.last_provider = Some(normalize_provider_for_model_selection(provider));
    save_auth(&store)
}

pub(crate) fn save_github_copilot_config(domain: &str) -> Result<()> {
    let mut store = load_auth();
    store.providers.insert(
        "github-copilot".to_string(),
        AuthEntry::ProviderConfig {
            domain: normalize_github_domain(domain)?,
        },
    );
    save_auth(&store)
}

pub(super) fn save_oauth_state(
    provider: &str,
    access: String,
    refresh: String,
    expires: i64,
    extra: serde_json::Value,
) -> Result<()> {
    let mut store = load_auth();
    store.providers.insert(
        provider.to_string(),
        AuthEntry::OAuth {
            access,
            refresh,
            expires,
            extra,
        },
    );
    store.last_provider = Some(normalize_provider_for_model_selection(provider));
    save_auth(&store)
}

pub(crate) fn github_copilot_domain() -> Option<String> {
    let store = load_auth();
    match store.providers.get("github-copilot") {
        Some(AuthEntry::ProviderConfig { domain }) => Some(domain.clone()),
        Some(AuthEntry::OAuth { extra, .. }) => extra
            .get("domain")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        _ => None,
    }
}

pub(crate) fn github_copilot_api_base_url() -> String {
    let default = "https://api.githubcopilot.com".to_string();
    let store = load_auth();
    match store.providers.get("github-copilot") {
        Some(AuthEntry::OAuth { extra, .. }) => extra
            .get("copilot_api_base_url")
            .and_then(|value| value.as_str())
            .map(ToString::to_string)
            .unwrap_or(default),
        _ => default,
    }
}

pub(crate) fn github_copilot_runtime_headers() -> std::collections::HashMap<String, String> {
    crate::oauth::github_copilot::github_copilot_runtime_headers()
}

pub(crate) fn github_copilot_cached_models() -> Vec<String> {
    github_copilot_status().cached_models
}

pub(crate) fn github_copilot_status() -> GithubCopilotStatus {
    let store = load_auth();
    let Some(entry) = store.providers.get("github-copilot") else {
        return GithubCopilotStatus::default();
    };

    match entry {
        AuthEntry::ProviderConfig { domain } => GithubCopilotStatus {
            authority: Some(domain.clone()),
            ..GithubCopilotStatus::default()
        },
        AuthEntry::OAuth { extra, .. } => GithubCopilotStatus {
            authority: extra
                .get("domain")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            login: extra
                .get("login")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            api_base_url: extra
                .get("copilot_api_base_url")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            cached_models: extra
                .get("copilot_models")
                .and_then(|value| value.as_array())
                .map(|models| {
                    models
                        .iter()
                        .filter_map(|value| value.as_str().map(ToString::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            github_access_expires_at: extra
                .get("github_access_expires_at")
                .and_then(|value| value.as_i64()),
            github_refresh_expires_at: extra
                .get("github_refresh_expires_at")
                .and_then(|value| value.as_i64()),
            copilot_expires_at: extra
                .get("copilot_expires_at")
                .and_then(|value| value.as_i64()),
            has_oauth: true,
        },
        AuthEntry::ApiKey { .. } => GithubCopilotStatus::default(),
    }
}

pub(crate) fn normalize_github_domain(input: &str) -> Result<String> {
    crate::oauth::github_copilot::normalize_authority(input)
}

pub(crate) fn configured_providers() -> Vec<String> {
    let store = load_auth();
    let mut providers = store.providers.keys().cloned().collect::<Vec<_>>();
    providers.sort();
    providers
}
