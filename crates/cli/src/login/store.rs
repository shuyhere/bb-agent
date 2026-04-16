use super::*;

#[derive(Serialize, Deserialize, Default)]
pub(super) struct AuthStore {
    #[serde(default)]
    pub(super) last_provider: Option<String>,
    #[serde(default)]
    pub(super) active_auth_methods: HashMap<String, ProviderAuthMethod>,
    #[serde(flatten)]
    pub(super) providers: HashMap<String, AuthEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
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

/// Snapshot of the persisted GitHub Copilot login state used by session info,
/// auth menus, and post-login status messages.
///
/// The authority may come from either the dedicated provider config entry or
/// the last OAuth payload, while cached model/API fields are only populated
/// once an OAuth login has completed successfully.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct GithubCopilotStatus {
    pub authority: Option<String>,
    pub login: Option<String>,
    pub api_base_url: Option<String>,
    pub cached_models: Vec<String>,
    pub github_access_expires_at: Option<i64>,
    pub github_refresh_expires_at: Option<i64>,
    pub copilot_expires_at: Option<i64>,
    pub has_oauth: bool,
}

fn normalized_auth_provider(provider: &str) -> String {
    normalize_provider_for_model_selection(provider)
}

pub(super) fn provider_storage_key(provider: &str, method: ProviderAuthMethod) -> String {
    let provider = normalized_auth_provider(provider);
    match (provider.as_str(), method) {
        ("openai", ProviderAuthMethod::OAuth) => "openai-codex".to_string(),
        ("openai", ProviderAuthMethod::ApiKey) => "openai".to_string(),
        ("anthropic", ProviderAuthMethod::OAuth) => "anthropic-oauth".to_string(),
        ("anthropic", ProviderAuthMethod::ApiKey) => "anthropic".to_string(),
        ("github-copilot", ProviderAuthMethod::OAuth) => "github-copilot".to_string(),
        (_, ProviderAuthMethod::ApiKey) => provider,
        (_, ProviderAuthMethod::OAuth) => provider,
    }
}

fn auth_entry_matches_method(entry: &AuthEntry, method: ProviderAuthMethod) -> bool {
    match (entry, method) {
        (AuthEntry::ApiKey { key }, ProviderAuthMethod::ApiKey) => !key.trim().is_empty(),
        (AuthEntry::OAuth { access, .. }, ProviderAuthMethod::OAuth) => !access.trim().is_empty(),
        _ => false,
    }
}

pub(super) fn stored_auth_entry_for_method<'a>(
    store: &'a AuthStore,
    provider: &str,
    method: ProviderAuthMethod,
) -> Option<&'a AuthEntry> {
    let normalized = normalized_auth_provider(provider);
    let key = provider_storage_key(&normalized, method);
    if let Some(entry) = store.providers.get(&key)
        && auth_entry_matches_method(entry, method)
    {
        return Some(entry);
    }

    if normalized == "anthropic"
        && method == ProviderAuthMethod::OAuth
        && !store.providers.contains_key("anthropic-oauth")
        && let Some(entry) = store.providers.get("anthropic")
        && auth_entry_matches_method(entry, method)
    {
        return Some(entry);
    }

    None
}

pub(super) fn stored_auth_methods_for_store(
    store: &AuthStore,
    provider: &str,
) -> Vec<ProviderAuthMethod> {
    let mut methods = Vec::new();
    for method in [ProviderAuthMethod::OAuth, ProviderAuthMethod::ApiKey] {
        if stored_auth_entry_for_method(store, provider, method).is_some() {
            methods.push(method);
        }
    }
    methods
}

pub(crate) fn stored_auth_methods(provider: &str) -> Vec<ProviderAuthMethod> {
    let store = load_auth();
    stored_auth_methods_for_store(&store, provider)
}

pub(crate) fn active_auth_method(provider: &str) -> Option<ProviderAuthMethod> {
    let store = load_auth();
    let normalized = normalized_auth_provider(provider);
    if let Some(method) = store.active_auth_methods.get(&normalized).copied()
        && stored_auth_entry_for_method(&store, &normalized, method).is_some()
    {
        return Some(method);
    }

    let methods = stored_auth_methods_for_store(&store, &normalized);
    if methods.len() == 1 {
        return methods.first().copied();
    }
    if methods.contains(&ProviderAuthMethod::ApiKey) {
        return Some(ProviderAuthMethod::ApiKey);
    }
    methods.first().copied()
}

pub(crate) fn set_active_auth_method(provider: &str, method: ProviderAuthMethod) -> Result<bool> {
    let mut store = load_auth();
    let normalized = normalized_auth_provider(provider);
    if stored_auth_entry_for_method(&store, &normalized, method).is_none() {
        return Ok(false);
    }
    store.active_auth_methods.insert(normalized.clone(), method);
    store.last_provider = Some(normalized);
    save_auth(&store)?;
    Ok(true)
}

fn migrate_legacy_anthropic_oauth_if_needed(store: &mut AuthStore) {
    if store.providers.contains_key("anthropic-oauth") {
        return;
    }
    let should_migrate = matches!(
        store.providers.get("anthropic"),
        Some(AuthEntry::OAuth { access, .. }) if !access.trim().is_empty()
    );
    if should_migrate && let Some(entry) = store.providers.remove("anthropic") {
        store.providers.insert("anthropic-oauth".to_string(), entry);
    }
}

pub(crate) fn remove_auth(provider: &str) -> Result<bool> {
    let mut store = load_auth();
    let normalized = normalized_auth_provider(provider);
    let mut removed = false;
    for key in [
        provider_storage_key(&normalized, ProviderAuthMethod::ApiKey),
        provider_storage_key(&normalized, ProviderAuthMethod::OAuth),
    ] {
        removed |= store.providers.remove(&key).is_some();
    }
    if normalized == "anthropic" {
        removed |= store.providers.remove("anthropic-oauth").is_some();
    }
    if removed {
        store.active_auth_methods.remove(&normalized);
        if store.last_provider.as_deref() == Some(normalized.as_str()) {
            store.last_provider = None;
        }
        save_auth(&store)?;
    }
    Ok(removed)
}

/// Path to the shared CLI auth store used by both `bb login` and the TUI
/// auth flows.
///
/// Example:
/// - on Linux this typically resolves under `~/.bb-agent/auth.json`
pub(crate) fn auth_path() -> PathBuf {
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

impl std::fmt::Debug for AuthStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let provider_names = self.providers.keys().cloned().collect::<Vec<_>>();
        f.debug_struct("AuthStore")
            .field("last_provider", &self.last_provider)
            .field("active_auth_methods", &self.active_auth_methods)
            .field("providers", &provider_names)
            .finish()
    }
}

impl std::fmt::Debug for AuthEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApiKey { .. } => f
                .debug_struct("ApiKey")
                .field("key", &"[REDACTED]")
                .finish(),
            Self::OAuth {
                expires, extra: _, ..
            } => f
                .debug_struct("OAuth")
                .field("access", &"[REDACTED]")
                .field("refresh", &"[REDACTED]")
                .field("expires", expires)
                .field("extra", &"[REDACTED]")
                .finish(),
            Self::ProviderConfig { domain } => f
                .debug_struct("ProviderConfig")
                .field("domain", domain)
                .finish(),
        }
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
    if normalized_auth_provider(provider) == "anthropic" {
        migrate_legacy_anthropic_oauth_if_needed(&mut store);
    }
    store.providers.insert(
        provider_storage_key(provider, ProviderAuthMethod::ApiKey),
        AuthEntry::ApiKey { key },
    );
    let normalized = normalized_auth_provider(provider);
    store.last_provider = Some(normalized.clone());
    store
        .active_auth_methods
        .insert(normalized, ProviderAuthMethod::ApiKey);
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
    if normalized_auth_provider(provider) == "anthropic" {
        migrate_legacy_anthropic_oauth_if_needed(&mut store);
    }
    store.providers.insert(
        provider_storage_key(provider, ProviderAuthMethod::OAuth),
        AuthEntry::OAuth {
            access,
            refresh,
            expires,
            extra,
        },
    );
    let normalized = normalized_auth_provider(provider);
    store.last_provider = Some(normalized.clone());
    store
        .active_auth_methods
        .insert(normalized, ProviderAuthMethod::OAuth);
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

/// Read the current GitHub Copilot login snapshot from the auth store.
///
/// This intentionally merges the provider-config-only case (enterprise host
/// saved but no OAuth token yet) with the full OAuth case so session info and
/// login UIs can explain exactly what has been configured.
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
    let mut providers = Vec::new();
    for provider in known_providers().iter().map(|(name, _, _)| *name) {
        let normalized = normalized_auth_provider(provider);
        let has_auth = !stored_auth_methods_for_store(&store, &normalized).is_empty();
        let has_config =
            normalized == "github-copilot" && store.providers.contains_key("github-copilot");
        if (has_auth || has_config) && !providers.iter().any(|existing| existing == &normalized) {
            providers.push(normalized);
        }
    }
    providers.sort();
    providers
}

#[cfg(test)]
mod tests {
    use super::{
        AuthEntry, AuthStore, stored_auth_entry_for_method, stored_auth_methods_for_store,
    };
    use crate::login::ProviderAuthMethod;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn auth_entry_debug_redacts_secret_fields() {
        let entry = AuthEntry::OAuth {
            access: "access-secret".to_string(),
            refresh: "refresh-secret".to_string(),
            expires: 123,
            extra: json!({"copilot_token": "runtime-secret"}),
        };

        let rendered = format!("{entry:?}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("access-secret"));
        assert!(!rendered.contains("refresh-secret"));
        assert!(!rendered.contains("runtime-secret"));
    }

    #[test]
    fn auth_store_debug_lists_provider_names_without_values() {
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            AuthEntry::ApiKey {
                key: "api-secret".to_string(),
            },
        );
        let store = AuthStore {
            last_provider: Some("openai".to_string()),
            active_auth_methods: HashMap::new(),
            providers,
        };

        let rendered = format!("{store:?}");
        assert!(rendered.contains("openai"));
        assert!(!rendered.contains("api-secret"));
    }

    #[test]
    fn stored_auth_methods_distinguish_anthropic_api_key_and_oauth() {
        let mut providers = HashMap::new();
        providers.insert(
            "anthropic".to_string(),
            AuthEntry::ApiKey {
                key: "api-secret".to_string(),
            },
        );
        providers.insert(
            "anthropic-oauth".to_string(),
            AuthEntry::OAuth {
                access: "oauth-secret".to_string(),
                refresh: "refresh-secret".to_string(),
                expires: i64::MAX,
                extra: json!({}),
            },
        );
        let store = AuthStore {
            last_provider: Some("anthropic".to_string()),
            active_auth_methods: HashMap::from([(
                "anthropic".to_string(),
                ProviderAuthMethod::OAuth,
            )]),
            providers,
        };

        assert_eq!(
            stored_auth_methods_for_store(&store, "anthropic"),
            vec![ProviderAuthMethod::OAuth, ProviderAuthMethod::ApiKey]
        );
        assert!(
            stored_auth_entry_for_method(&store, "anthropic", ProviderAuthMethod::ApiKey).is_some()
        );
        assert!(
            stored_auth_entry_for_method(&store, "anthropic", ProviderAuthMethod::OAuth).is_some()
        );
    }
}
