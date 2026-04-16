use super::*;

const AUTH_STORE_VERSION: u32 = 2;

#[derive(Clone, Serialize, Deserialize, Default)]
pub(super) struct AuthStore {
    #[serde(default)]
    pub(super) version: u32,
    #[serde(default)]
    pub(super) last_provider: Option<String>,
    #[serde(default)]
    pub(super) active_auth_methods: HashMap<String, ProviderAuthMethod>,
    #[serde(default)]
    pub(super) active_auth_profiles: HashMap<String, String>,
    #[serde(default)]
    pub(super) profiles: HashMap<String, Vec<AuthProfile>>,
    #[serde(default)]
    pub(super) provider_configs: HashMap<String, ProviderConfigRecord>,
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

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct AuthProfile {
    pub(super) id: String,
    pub(super) method: ProviderAuthMethod,
    #[serde(default)]
    pub(super) created_at_ms: Option<i64>,
    #[serde(default)]
    pub(super) updated_at_ms: Option<i64>,
    #[serde(flatten)]
    pub(super) entry: AuthEntry,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub(super) struct ProviderConfigRecord {
    pub(super) domain: String,
    #[serde(default)]
    pub(super) created_at_ms: Option<i64>,
    #[serde(default)]
    pub(super) updated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredAuthProfileSummary {
    pub profile_id: String,
    pub method: ProviderAuthMethod,
    pub account_label: Option<String>,
    pub authority: Option<String>,
    pub configured_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub active: bool,
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

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
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

fn auth_profile_matches(profile: &AuthProfile) -> bool {
    auth_entry_matches_method(&profile.entry, profile.method)
}

fn profile_sort_key(profile: &AuthProfile) -> i64 {
    profile
        .updated_at_ms
        .or(profile.created_at_ms)
        .unwrap_or_default()
}

fn auth_entry_account_label(provider: &str, entry: &AuthEntry) -> Option<String> {
    let normalized = normalized_auth_provider(provider);
    match entry {
        AuthEntry::OAuth { extra, .. } => match normalized.as_str() {
            "github-copilot" => extra
                .get("login")
                .and_then(|value| value.as_str())
                .map(ToString::to_string),
            _ => extra
                .get("accountId")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .or_else(|| {
                    extra
                        .get("login")
                        .and_then(|value| value.as_str())
                        .map(ToString::to_string)
                }),
        },
        _ => None,
    }
}

fn auth_entry_authority(provider: &str, entry: &AuthEntry) -> Option<String> {
    let normalized = normalized_auth_provider(provider);
    match (normalized.as_str(), entry) {
        ("github-copilot", AuthEntry::OAuth { extra, .. }) => extra
            .get("domain")
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        _ => None,
    }
}

fn oauth_identity_from_entry(provider: &str, entry: &AuthEntry) -> Option<String> {
    let normalized = normalized_auth_provider(provider);
    let AuthEntry::OAuth { extra, .. } = entry else {
        return None;
    };

    match normalized.as_str() {
        "openai" | "anthropic" => extra
            .get("accountId")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("account:{value}")),
        "github-copilot" => {
            let authority = extra
                .get("domain")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty());
            let login = extra
                .get("login")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty());
            match (authority, login) {
                (Some(authority), Some(login)) => {
                    Some(format!("authority:{authority}|login:{login}"))
                }
                (Some(authority), None) => Some(format!("authority:{authority}")),
                (None, Some(login)) => Some(format!("login:{login}")),
                (None, None) => None,
            }
        }
        _ => extra
            .get("accountId")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("account:{value}"))
            .or_else(|| {
                extra
                    .get("login")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| format!("login:{value}"))
            }),
    }
}

fn best_profile_index_for_method(
    profiles: &[AuthProfile],
    method: ProviderAuthMethod,
    active_profile_id: Option<&str>,
) -> Option<usize> {
    if let Some(active_profile_id) = active_profile_id
        && let Some((idx, _)) = profiles.iter().enumerate().find(|(_, profile)| {
            profile.id == active_profile_id
                && profile.method == method
                && auth_profile_matches(profile)
        })
    {
        return Some(idx);
    }

    profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| profile.method == method && auth_profile_matches(profile))
        .max_by_key(|(_, profile)| profile_sort_key(profile))
        .map(|(idx, _)| idx)
}

pub(super) fn stored_auth_profile_for_method<'a>(
    store: &'a AuthStore,
    provider: &str,
    method: ProviderAuthMethod,
) -> Option<&'a AuthProfile> {
    let normalized = normalized_auth_provider(provider);
    let profiles = store.profiles.get(&normalized)?;
    let active_profile_id = store
        .active_auth_profiles
        .get(&normalized)
        .map(String::as_str);
    let idx = best_profile_index_for_method(profiles, method, active_profile_id)?;
    profiles.get(idx)
}

#[cfg(test)]
pub(super) fn stored_auth_entry_for_method<'a>(
    store: &'a AuthStore,
    provider: &str,
    method: ProviderAuthMethod,
) -> Option<&'a AuthEntry> {
    stored_auth_profile_for_method(store, provider, method).map(|profile| &profile.entry)
}

pub(super) fn stored_auth_methods_for_store(
    store: &AuthStore,
    provider: &str,
) -> Vec<ProviderAuthMethod> {
    let normalized = normalized_auth_provider(provider);
    let Some(profiles) = store.profiles.get(&normalized) else {
        return Vec::new();
    };

    let mut methods = Vec::new();
    for method in [ProviderAuthMethod::OAuth, ProviderAuthMethod::ApiKey] {
        if profiles
            .iter()
            .any(|profile| profile.method == method && auth_profile_matches(profile))
        {
            methods.push(method);
        }
    }
    methods
}

fn stored_auth_profiles_for_store(
    store: &AuthStore,
    provider: &str,
) -> Vec<StoredAuthProfileSummary> {
    let normalized = normalized_auth_provider(provider);
    let active_profile_id = store
        .active_auth_profiles
        .get(&normalized)
        .map(String::as_str);
    let mut profiles = store
        .profiles
        .get(&normalized)
        .into_iter()
        .flatten()
        .filter(|profile| auth_profile_matches(profile))
        .map(|profile| StoredAuthProfileSummary {
            profile_id: profile.id.clone(),
            method: profile.method,
            account_label: auth_entry_account_label(&normalized, &profile.entry),
            authority: auth_entry_authority(&normalized, &profile.entry),
            configured_at_ms: profile.created_at_ms,
            updated_at_ms: profile.updated_at_ms,
            active: active_profile_id == Some(profile.id.as_str()),
        })
        .collect::<Vec<_>>();
    profiles.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
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
            .then_with(|| left.profile_id.cmp(&right.profile_id))
    });
    profiles
}

pub(crate) fn stored_auth_methods(provider: &str) -> Vec<ProviderAuthMethod> {
    let store = load_auth();
    stored_auth_methods_for_store(&store, provider)
}

pub(crate) fn stored_auth_profiles(provider: &str) -> Vec<StoredAuthProfileSummary> {
    let store = load_auth();
    stored_auth_profiles_for_store(&store, provider)
}

pub(super) fn stored_auth_profile_by_id<'a>(
    store: &'a AuthStore,
    provider: &str,
    profile_id: &str,
) -> Option<&'a AuthProfile> {
    let normalized = normalized_auth_provider(provider);
    store
        .profiles
        .get(&normalized)
        .and_then(|profiles| profiles.iter().find(|profile| profile.id == profile_id))
}

pub(crate) fn active_auth_method(provider: &str) -> Option<ProviderAuthMethod> {
    let store = load_auth();
    let normalized = normalized_auth_provider(provider);
    if let Some(active_profile_id) = store.active_auth_profiles.get(&normalized)
        && let Some(profile) = store.profiles.get(&normalized).and_then(|profiles| {
            profiles
                .iter()
                .find(|profile| profile.id == *active_profile_id)
        })
        && auth_profile_matches(profile)
    {
        return Some(profile.method);
    }

    if let Some(method) = store.active_auth_methods.get(&normalized).copied()
        && stored_auth_profile_for_method(&store, &normalized, method).is_some()
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

pub(crate) fn set_active_auth_profile(provider: &str, profile_id: &str) -> Result<bool> {
    let mut store = load_auth();
    let normalized = normalized_auth_provider(provider);
    let Some(profile) = stored_auth_profile_by_id(&store, &normalized, profile_id).cloned() else {
        return Ok(false);
    };
    if !auth_profile_matches(&profile) {
        return Ok(false);
    }
    store
        .active_auth_profiles
        .insert(normalized.clone(), profile.id.clone());
    store
        .active_auth_methods
        .insert(normalized.clone(), profile.method);
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

fn legacy_provider_and_method(
    key: &str,
    entry: &AuthEntry,
) -> Option<(String, ProviderAuthMethod)> {
    match entry {
        AuthEntry::ApiKey { .. } => {
            Some((normalized_auth_provider(key), ProviderAuthMethod::ApiKey))
        }
        AuthEntry::OAuth { .. } => Some((normalized_auth_provider(key), ProviderAuthMethod::OAuth)),
        AuthEntry::ProviderConfig { .. } => None,
    }
}

fn repair_active_auth_selections(store: &mut AuthStore) {
    let mut providers = store.profiles.keys().cloned().collect::<Vec<_>>();
    providers.extend(store.active_auth_methods.keys().cloned());
    providers.extend(store.active_auth_profiles.keys().cloned());
    providers.sort();
    providers.dedup();

    for provider in providers {
        let selected = if let Some(active_profile_id) = store.active_auth_profiles.get(&provider) {
            store.profiles.get(&provider).and_then(|profiles| {
                profiles
                    .iter()
                    .find(|profile| {
                        profile.id == *active_profile_id && auth_profile_matches(profile)
                    })
                    .map(|profile| (profile.method, profile.id.clone()))
            })
        } else {
            None
        }
        .or_else(|| {
            store
                .active_auth_methods
                .get(&provider)
                .copied()
                .and_then(|method| {
                    store.profiles.get(&provider).and_then(|profiles| {
                        best_profile_index_for_method(profiles, method, None)
                            .and_then(|idx| profiles.get(idx))
                            .map(|profile| (method, profile.id.clone()))
                    })
                })
        })
        .or_else(|| {
            [ProviderAuthMethod::ApiKey, ProviderAuthMethod::OAuth]
                .into_iter()
                .find_map(|method| {
                    store.profiles.get(&provider).and_then(|profiles| {
                        best_profile_index_for_method(profiles, method, None)
                            .and_then(|idx| profiles.get(idx))
                            .map(|profile| (method, profile.id.clone()))
                    })
                })
        });

        if let Some((method, profile_id)) = selected {
            store.active_auth_methods.insert(provider.clone(), method);
            store
                .active_auth_profiles
                .insert(provider.clone(), profile_id);
        } else {
            store.active_auth_methods.remove(&provider);
            store.active_auth_profiles.remove(&provider);
        }
    }

    store.last_provider = store.last_provider.as_deref().map(normalized_auth_provider);
}

fn migrate_loaded_store(store: &mut AuthStore) {
    migrate_legacy_anthropic_oauth_if_needed(store);

    let legacy_entries = std::mem::take(&mut store.providers);
    for (key, entry) in legacy_entries {
        match entry {
            AuthEntry::ProviderConfig { domain } => {
                if key == "github-copilot" && !store.provider_configs.contains_key(&key) {
                    store.provider_configs.insert(
                        key,
                        ProviderConfigRecord {
                            domain,
                            created_at_ms: None,
                            updated_at_ms: None,
                        },
                    );
                }
            }
            other => {
                let Some((provider, method)) = legacy_provider_and_method(&key, &other) else {
                    continue;
                };
                let profiles = store.profiles.entry(provider).or_default();
                if profiles.iter().all(|profile| {
                    profile.method != method
                        || oauth_identity_from_entry(&key, &profile.entry)
                            != oauth_identity_from_entry(&key, &other)
                }) {
                    profiles.push(AuthProfile {
                        id: format!("legacy:{key}:{}", profiles.len()),
                        method,
                        created_at_ms: None,
                        updated_at_ms: None,
                        entry: other,
                    });
                }
            }
        }
    }

    repair_active_auth_selections(store);
    store.providers.clear();
    store.version = AUTH_STORE_VERSION;
}

pub(crate) fn remove_auth(provider: &str) -> Result<bool> {
    let mut store = load_auth();
    let normalized = normalized_auth_provider(provider);
    let removed_profiles = store.profiles.remove(&normalized).is_some();
    let removed_config = if normalized == "github-copilot" {
        store.provider_configs.remove("github-copilot").is_some()
    } else {
        false
    };
    let removed = removed_profiles || removed_config;
    if removed {
        store.active_auth_methods.remove(&normalized);
        store.active_auth_profiles.remove(&normalized);
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
    let mut store = if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => AuthStore::default(),
        }
    } else {
        AuthStore::default()
    };
    migrate_loaded_store(&mut store);
    store
}

impl std::fmt::Debug for AuthStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut provider_names = self.profiles.keys().cloned().collect::<Vec<_>>();
        provider_names.extend(self.provider_configs.keys().cloned());
        provider_names.extend(self.providers.keys().cloned());
        provider_names.sort();
        provider_names.dedup();
        f.debug_struct("AuthStore")
            .field("version", &self.version)
            .field("last_provider", &self.last_provider)
            .field("active_auth_methods", &self.active_auth_methods)
            .field("active_auth_profiles", &self.active_auth_profiles)
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

    let mut persisted = store.clone();
    migrate_loaded_store(&mut persisted);
    let content = serde_json::to_string_pretty(&persisted)?;
    std::fs::write(&path, &content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

fn upsert_api_key_profile(store: &mut AuthStore, provider: &str, key: String) -> String {
    let normalized = normalized_auth_provider(provider);
    let active_profile_id = store.active_auth_profiles.get(&normalized).cloned();
    let profiles = store.profiles.entry(normalized.clone()).or_default();
    let idx = best_profile_index_for_method(
        profiles,
        ProviderAuthMethod::ApiKey,
        active_profile_id.as_deref(),
    );
    let timestamp = now_ms();
    if let Some(idx) = idx {
        let profile = &mut profiles[idx];
        profile.entry = AuthEntry::ApiKey { key };
        profile.updated_at_ms = Some(timestamp);
        return profile.id.clone();
    }

    let profile_id = format!(
        "{}-{}",
        provider_storage_key(&normalized, ProviderAuthMethod::ApiKey),
        uuid::Uuid::new_v4()
    );
    profiles.push(AuthProfile {
        id: profile_id.clone(),
        method: ProviderAuthMethod::ApiKey,
        created_at_ms: Some(timestamp),
        updated_at_ms: Some(timestamp),
        entry: AuthEntry::ApiKey { key },
    });
    profile_id
}

fn select_oauth_profile_index_for_update(
    profiles: &[AuthProfile],
    provider: &str,
    active_profile_id: Option<&str>,
    new_entry: &AuthEntry,
) -> Option<usize> {
    let identity = oauth_identity_from_entry(provider, new_entry);
    if let Some(identity) = identity.as_deref() {
        return profiles.iter().enumerate().find_map(|(idx, profile)| {
            (profile.method == ProviderAuthMethod::OAuth
                && oauth_identity_from_entry(provider, &profile.entry).as_deref() == Some(identity))
            .then_some(idx)
        });
    }

    if let Some(active_profile_id) = active_profile_id
        && let Some((idx, _)) = profiles.iter().enumerate().find(|(_, profile)| {
            profile.id == active_profile_id
                && profile.method == ProviderAuthMethod::OAuth
                && auth_profile_matches(profile)
        })
    {
        return Some(idx);
    }

    let oauth_profiles = profiles
        .iter()
        .enumerate()
        .filter(|(_, profile)| {
            profile.method == ProviderAuthMethod::OAuth && auth_profile_matches(profile)
        })
        .collect::<Vec<_>>();
    if oauth_profiles.len() == 1 {
        return oauth_profiles.first().map(|(idx, _)| *idx);
    }

    None
}

fn upsert_oauth_profile(
    store: &mut AuthStore,
    provider: &str,
    access: String,
    refresh: String,
    expires: i64,
    extra: serde_json::Value,
) -> String {
    let normalized = normalized_auth_provider(provider);
    let active_profile_id = store.active_auth_profiles.get(&normalized).cloned();
    let new_entry = AuthEntry::OAuth {
        access,
        refresh,
        expires,
        extra,
    };
    let profiles = store.profiles.entry(normalized.clone()).or_default();
    let idx = select_oauth_profile_index_for_update(
        profiles,
        &normalized,
        active_profile_id.as_deref(),
        &new_entry,
    );
    let timestamp = now_ms();
    if let Some(idx) = idx {
        let profile = &mut profiles[idx];
        profile.entry = new_entry;
        profile.updated_at_ms = Some(timestamp);
        return profile.id.clone();
    }

    let profile_id = format!(
        "{}-{}",
        provider_storage_key(&normalized, ProviderAuthMethod::OAuth),
        uuid::Uuid::new_v4()
    );
    profiles.push(AuthProfile {
        id: profile_id.clone(),
        method: ProviderAuthMethod::OAuth,
        created_at_ms: Some(timestamp),
        updated_at_ms: Some(timestamp),
        entry: new_entry,
    });
    profile_id
}

pub(crate) fn save_api_key(provider: &str, key: String) -> Result<()> {
    let mut store = load_auth();
    let normalized = normalized_auth_provider(provider);
    let profile_id = upsert_api_key_profile(&mut store, provider, key);
    store.last_provider = Some(normalized.clone());
    store
        .active_auth_methods
        .insert(normalized.clone(), ProviderAuthMethod::ApiKey);
    store.active_auth_profiles.insert(normalized, profile_id);
    save_auth(&store)
}

pub(crate) fn save_github_copilot_config(domain: &str) -> Result<()> {
    let mut store = load_auth();
    let timestamp = now_ms();
    let normalized = normalize_github_domain(domain)?;
    match store.provider_configs.get_mut("github-copilot") {
        Some(config) => {
            config.domain = normalized;
            config.updated_at_ms = Some(timestamp);
        }
        None => {
            store.provider_configs.insert(
                "github-copilot".to_string(),
                ProviderConfigRecord {
                    domain: normalized,
                    created_at_ms: Some(timestamp),
                    updated_at_ms: Some(timestamp),
                },
            );
        }
    }
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
    let normalized = normalized_auth_provider(provider);
    let profile_id = upsert_oauth_profile(&mut store, provider, access, refresh, expires, extra);
    store.last_provider = Some(normalized.clone());
    store
        .active_auth_methods
        .insert(normalized.clone(), ProviderAuthMethod::OAuth);
    store.active_auth_profiles.insert(normalized, profile_id);
    save_auth(&store)
}

pub(crate) fn github_copilot_domain() -> Option<String> {
    let store = load_auth();
    store
        .provider_configs
        .get("github-copilot")
        .map(|config| config.domain.clone())
        .or_else(|| {
            stored_auth_profile_for_method(&store, "github-copilot", ProviderAuthMethod::OAuth)
                .and_then(|profile| auth_entry_authority("github-copilot", &profile.entry))
        })
}

pub(crate) fn github_copilot_api_base_url() -> String {
    let default = "https://api.githubcopilot.com".to_string();
    let store = load_auth();
    match stored_auth_profile_for_method(&store, "github-copilot", ProviderAuthMethod::OAuth) {
        Some(AuthProfile {
            entry: AuthEntry::OAuth { extra, .. },
            ..
        }) => extra
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
    let config_authority = store
        .provider_configs
        .get("github-copilot")
        .map(|config| config.domain.clone());
    let Some(profile) =
        stored_auth_profile_for_method(&store, "github-copilot", ProviderAuthMethod::OAuth)
    else {
        return GithubCopilotStatus {
            authority: config_authority,
            ..GithubCopilotStatus::default()
        };
    };

    match &profile.entry {
        AuthEntry::OAuth { extra, .. } => GithubCopilotStatus {
            authority: extra
                .get("domain")
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
                .or(config_authority),
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
        _ => GithubCopilotStatus {
            authority: config_authority,
            ..GithubCopilotStatus::default()
        },
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
            normalized == "github-copilot" && store.provider_configs.contains_key("github-copilot");
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
        AUTH_STORE_VERSION, AuthEntry, AuthProfile, AuthStore, ProviderConfigRecord, load_auth,
        save_api_key, save_auth, save_oauth_state, stored_auth_entry_for_method,
        stored_auth_methods_for_store, stored_auth_profiles,
    };
    use crate::login::ProviderAuthMethod;
    use serde_json::json;
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
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
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
        let store = AuthStore {
            last_provider: Some("openai".to_string()),
            active_auth_methods: HashMap::new(),
            active_auth_profiles: HashMap::new(),
            profiles: HashMap::from([(
                "openai".to_string(),
                vec![AuthProfile {
                    id: "openai-profile".to_string(),
                    method: ProviderAuthMethod::ApiKey,
                    created_at_ms: Some(123),
                    updated_at_ms: Some(123),
                    entry: AuthEntry::ApiKey {
                        key: "api-secret".to_string(),
                    },
                }],
            )]),
            provider_configs: HashMap::new(),
            providers: HashMap::new(),
            version: AUTH_STORE_VERSION,
        };

        let rendered = format!("{store:?}");
        assert!(rendered.contains("openai"));
        assert!(!rendered.contains("api-secret"));
    }

    #[test]
    fn stored_auth_methods_distinguish_anthropic_api_key_and_oauth() {
        let store = AuthStore {
            last_provider: Some("anthropic".to_string()),
            active_auth_methods: HashMap::from([(
                "anthropic".to_string(),
                ProviderAuthMethod::OAuth,
            )]),
            active_auth_profiles: HashMap::from([(
                "anthropic".to_string(),
                "anthropic-oauth-profile".to_string(),
            )]),
            profiles: HashMap::from([(
                "anthropic".to_string(),
                vec![
                    AuthProfile {
                        id: "anthropic-api-profile".to_string(),
                        method: ProviderAuthMethod::ApiKey,
                        created_at_ms: Some(10),
                        updated_at_ms: Some(10),
                        entry: AuthEntry::ApiKey {
                            key: "api-secret".to_string(),
                        },
                    },
                    AuthProfile {
                        id: "anthropic-oauth-profile".to_string(),
                        method: ProviderAuthMethod::OAuth,
                        created_at_ms: Some(20),
                        updated_at_ms: Some(20),
                        entry: AuthEntry::OAuth {
                            access: "oauth-secret".to_string(),
                            refresh: "refresh-secret".to_string(),
                            expires: i64::MAX,
                            extra: json!({}),
                        },
                    },
                ],
            )]),
            provider_configs: HashMap::new(),
            providers: HashMap::new(),
            version: AUTH_STORE_VERSION,
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

    #[test]
    fn save_oauth_state_keeps_distinct_openai_accounts_with_timestamps() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set_path("HOME", home.path());

        save_oauth_state(
            "openai-codex",
            "oauth-access-1".to_string(),
            "refresh-1".to_string(),
            i64::MAX,
            json!({"accountId": "acct_primary"}),
        )
        .expect("save first openai oauth");
        save_oauth_state(
            "openai-codex",
            "oauth-access-2".to_string(),
            "refresh-2".to_string(),
            i64::MAX,
            json!({"accountId": "acct_secondary"}),
        )
        .expect("save second openai oauth");

        let store = load_auth();
        let profiles = stored_auth_profiles("openai");
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].account_label.as_deref(), Some("acct_secondary"));
        assert!(profiles[0].active);
        assert!(
            profiles
                .iter()
                .all(|profile| profile.configured_at_ms.is_some())
        );
        assert_eq!(
            stored_auth_methods_for_store(&store, "openai"),
            vec![ProviderAuthMethod::OAuth]
        );
    }

    #[test]
    fn save_api_key_reuses_existing_profile_and_updates_timestamp() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set_path("HOME", home.path());

        save_api_key("openrouter", "key-1".to_string()).expect("save first key");
        let first = stored_auth_profiles("openrouter");
        assert_eq!(first.len(), 1);
        let first_profile_id = first[0].profile_id.clone();

        save_api_key("openrouter", "key-2".to_string()).expect("save second key");
        let second = stored_auth_profiles("openrouter");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].profile_id, first_profile_id);
        assert!(second[0].updated_at_ms.is_some());
    }

    #[test]
    fn legacy_flat_auth_store_migrates_to_profiles() {
        let _lock = env_lock().lock().unwrap();
        let home = tempfile::tempdir().expect("home tempdir");
        let _home = EnvVarGuard::set_path("HOME", home.path());

        save_auth(&AuthStore {
            last_provider: Some("openai".to_string()),
            active_auth_methods: HashMap::from([("openai".to_string(), ProviderAuthMethod::OAuth)]),
            active_auth_profiles: HashMap::new(),
            profiles: HashMap::new(),
            provider_configs: HashMap::from([(
                "github-copilot".to_string(),
                ProviderConfigRecord {
                    domain: "github.example.com".to_string(),
                    created_at_ms: None,
                    updated_at_ms: None,
                },
            )]),
            providers: HashMap::from([(
                "openai-codex".to_string(),
                AuthEntry::OAuth {
                    access: "oauth-access".to_string(),
                    refresh: "refresh-token".to_string(),
                    expires: i64::MAX,
                    extra: json!({"accountId": "acct_test123"}),
                },
            )]),
            version: 0,
        })
        .expect("save auth");

        let migrated = load_auth();
        assert!(migrated.providers.is_empty());
        let profiles = stored_auth_profiles("openai");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].account_label.as_deref(), Some("acct_test123"));
        assert_eq!(profiles[0].configured_at_ms, None);
    }
}
