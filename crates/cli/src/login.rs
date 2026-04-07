use anyhow::Result;
use bb_core::config;
use bb_core::settings::Settings;
use bb_provider::registry::{Model, ModelRegistry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use crate::oauth::OAuthCredentials;

pub(crate) fn try_open_browser(url: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        return std::process::Command::new("open")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
    }

    #[cfg(not(target_os = "macos"))]
    {
        let has_launcher_hint = std::env::var_os("DISPLAY").is_some()
            || std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var_os("BROWSER").is_some();
        if !has_launcher_hint {
            return false;
        }

        std::process::Command::new("xdg-open")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AuthStore {
    #[serde(default)]
    last_provider: Option<String>,
    #[serde(flatten)]
    providers: HashMap<String, AuthEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum AuthEntry {
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

const KNOWN_PROVIDERS: &[(&str, &str, &str)] = &[
    (
        "anthropic",
        "ANTHROPIC_API_KEY",
        "https://console.anthropic.com/settings/keys",
    ),
    ("openai-codex", "", "https://chatgpt.com/"),
    ("github-copilot", "", "https://github.com/features/copilot"),
    (
        "openai",
        "OPENAI_API_KEY",
        "https://platform.openai.com/api-keys",
    ),
    (
        "google",
        "GOOGLE_API_KEY",
        "https://aistudio.google.com/app/apikey",
    ),
    ("groq", "GROQ_API_KEY", "https://console.groq.com/keys"),
    ("xai", "XAI_API_KEY", "https://console.x.ai/"),
    (
        "openrouter",
        "OPENROUTER_API_KEY",
        "https://openrouter.ai/settings/keys",
    ),
];

/// Providers that use OAuth instead of API key paste.
const OAUTH_PROVIDERS: &[&str] = &["anthropic", "openai-codex", "github-copilot"];

fn normalize_provider_for_model_selection(provider: &str) -> String {
    match provider {
        "openai-codex" => "openai".to_string(),
        other => other.to_string(),
    }
}

pub fn provider_meta(provider: &str) -> (&str, &str) {
    KNOWN_PROVIDERS
        .iter()
        .find(|(name, _, _)| *name == provider)
        .map(|(_, env_var, url)| (*env_var, *url))
        .unwrap_or(("API_KEY", ""))
}

pub(crate) fn provider_display_name(provider: &str) -> String {
    match provider {
        "anthropic" => "Claude Pro/Max".to_string(),
        "openai-codex" => "ChatGPT Plus/Pro (Codex)".to_string(),
        "github-copilot" => "GitHub Copilot".to_string(),
        "openai" => "OpenAI".to_string(),
        "google" => "Google Gemini".to_string(),
        "groq" => "Groq".to_string(),
        "xai" => "xAI".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        _ => provider.to_string(),
    }
}

pub(crate) fn provider_auth_method(provider: &str) -> &'static str {
    if OAUTH_PROVIDERS.contains(&provider) {
        "OAuth"
    } else {
        "API key"
    }
}

pub(crate) fn provider_login_hint(provider: &str) -> String {
    match provider {
        "openai-codex" => {
            "Requires ChatGPT Plus or Pro subscription. Uses browser OAuth, not OpenAI API keys."
                .to_string()
        }
        "anthropic" => {
            "Requires Claude Pro or Max subscription. Uses browser OAuth, not Anthropic API keys."
                .to_string()
        }
        "github-copilot" => {
            let target = github_copilot_domain().unwrap_or_else(|| "github.com".to_string());
            format!(
                "Uses GitHub device/browser auth, then exchanges the GitHub token for a Copilot runtime token. Supports github.com or GitHub Enterprise Server. Current target: {target}."
            )
        }
        other => {
            let (env_var, url) = provider_meta(other);
            if url.is_empty() {
                format!("Set {env_var} or paste an API key.")
            } else {
                format!("Get an API key from {url} or set {env_var}.")
            }
        }
    }
}

pub(crate) fn provider_oauth_variant(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" => Some("anthropic"),
        "openai" | "openai-codex" => Some("openai-codex"),
        "github-copilot" => Some("github-copilot"),
        _ => None,
    }
}

pub(crate) fn provider_api_key_variant(provider: &str) -> Option<&'static str> {
    match provider {
        "anthropic" => Some("anthropic"),
        "openai" | "openai-codex" => Some("openai"),
        "google" => Some("google"),
        "groq" => Some("groq"),
        "xai" => Some("xai"),
        "openrouter" => Some("openrouter"),
        _ => None,
    }
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

fn load_auth() -> AuthStore {
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

fn save_auth(store: &AuthStore) -> Result<()> {
    let path = auth_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(store)?;
    std::fs::write(&path, &content)?;

    // Restrict file permissions to owner-only (0600) on Unix.
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

fn save_oauth_state(
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
    if let Ok(url) = std::env::var("GH_COPILOT_API_URL")
        && !url.trim().is_empty()
    {
        return url;
    }
    if let Ok(url) = std::env::var("GITHUB_COPILOT_API_URL")
        && !url.trim().is_empty()
    {
        return url;
    }

    let store = load_auth();
    if let Some(AuthEntry::OAuth { extra, .. }) = store.providers.get("github-copilot")
        && let Some(url) = extra
            .get("copilot_api_base_url")
            .and_then(|value| value.as_str())
        && !url.trim().is_empty()
    {
        return url.to_string();
    }

    "https://api.githubcopilot.com".to_string()
}

pub(crate) fn github_copilot_runtime_headers() -> std::collections::HashMap<String, String> {
    crate::oauth::github_copilot::github_copilot_runtime_headers()
}

pub(crate) fn github_copilot_cached_models() -> Vec<String> {
    let store = load_auth();
    let Some(AuthEntry::OAuth { extra, .. }) = store.providers.get("github-copilot") else {
        return Vec::new();
    };
    extra
        .get("copilot_models")
        .and_then(|value| value.as_array())
        .map(|models| {
            models
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
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

pub(crate) fn auth_source_label(provider: &str) -> &'static str {
    match auth_source(provider) {
        Some(AuthSource::BbAuth) => "bb auth.json",
        Some(AuthSource::EnvVar) => "environment",
        None => "not configured",
    }
}

pub(crate) fn add_cached_github_copilot_models(
    registry: &mut bb_provider::registry::ModelRegistry,
) {
    for model_id in github_copilot_cached_models() {
        if registry.find("github-copilot", &model_id).is_none() {
            registry.add(bb_provider::registry::Model {
                id: model_id.clone(),
                name: model_id.clone(),
                provider: "github-copilot".to_string(),
                api: bb_provider::registry::ApiType::OpenaiCompletions,
                context_window: 128_000,
                max_tokens: 16_384,
                reasoning: true,
                base_url: Some(github_copilot_api_base_url()),
                cost: Default::default(),
            });
        }
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

pub(crate) async fn run_oauth_login(
    provider: &str,
    callbacks: crate::oauth::OAuthCallbacks,
) -> Result<()> {
    use crate::oauth;

    let creds = match provider {
        "anthropic" => oauth::login_anthropic(callbacks).await?,
        "openai-codex" => oauth::login_openai_codex(callbacks).await?,
        "github-copilot" => {
            let authority = github_copilot_domain().unwrap_or_else(|| "github.com".to_string());
            oauth::login_github_copilot(&authority, callbacks).await?
        }
        other => anyhow::bail!("No OAuth flow for provider: {other}"),
    };

    save_oauth_credentials(provider, &creds)
}

pub async fn handle_login(provider: Option<&str>) -> Result<()> {
    let provider = match provider {
        Some(p) => p.to_string(),
        None => {
            // Show provider selector
            println!("Available providers:");
            for (i, (name, _, _url)) in KNOWN_PROVIDERS.iter().enumerate() {
                let method_label = provider_auth_method(name);
                let status = get_provider_status(name);
                let hint = provider_login_hint(name);
                println!(
                    "  {}. {} ({}) {}\n     {}",
                    i + 1,
                    provider_display_name(name),
                    method_label,
                    status,
                    hint
                );
            }
            println!();
            print!("Select provider (number or name): ");
            std::io::stdout().flush()?;

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let input = input.trim();

            // Try as number
            if let Ok(num) = input.parse::<usize>() {
                if num >= 1 && num <= KNOWN_PROVIDERS.len() {
                    KNOWN_PROVIDERS[num - 1].0.to_string()
                } else {
                    anyhow::bail!("Invalid selection");
                }
            } else {
                input.to_string()
            }
        }
    };

    // ── GitHub Copilot host selection (OAuth backend not yet implemented) ──
    if provider == "github-copilot" {
        println!("GitHub Copilot sign-in target:");
        println!("  Press Enter for github.com, or enter your GitHub Enterprise Server domain.");
        print!("Domain [github.com]: ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let domain = if input.trim().is_empty() {
            "github.com".to_string()
        } else {
            normalize_github_domain(input.trim())?
        };

        save_github_copilot_config(&domain)?;
        println!("✓ Saved GitHub Copilot target: {domain}");
        return handle_oauth_login_cli(&provider).await;
    }

    // ── OAuth providers ─────────────────────────────────────────────
    if OAUTH_PROVIDERS.contains(&provider.as_str()) {
        return handle_oauth_login_cli(&provider).await;
    }

    // ── API-key providers (unchanged) ───────────────────────────────
    let (_, env_var, url) = KNOWN_PROVIDERS
        .iter()
        .find(|(name, _, _)| *name == provider)
        .copied()
        .unwrap_or((&provider, "API_KEY", ""));

    // Check if already have env var
    if let Ok(key) = std::env::var(env_var)
        && !key.is_empty()
    {
        println!(
            "✓ {} is already set via environment variable {}",
            provider, env_var
        );
        print!("Override with manual key? [y/N]: ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            return Ok(());
        }
    }

    if !url.is_empty() {
        println!("\nGet your API key from: {url}");
    }
    println!(
        "(Tip: you can also set {} environment variable instead)\n",
        env_var
    );

    print!("Enter API key for {provider}: ");
    std::io::stdout().flush()?;

    // Read key (ideally hidden, but simple for now)
    let mut key = String::new();
    std::io::stdin().read_line(&mut key)?;
    let key = key.trim().to_string();

    if key.is_empty() {
        println!("No key entered, aborting.");
        return Ok(());
    }

    save_api_key(&provider, key)?;

    println!("✓ API key saved for {provider}");
    println!("  Stored in: {}", auth_path().display());

    Ok(())
}

/// Run the OAuth browser flow from a plain terminal (non-TUI).
async fn handle_oauth_login_cli(provider: &str) -> Result<()> {
    use crate::oauth::OAuthCallbacks;

    println!("Starting OAuth login for {provider}…");

    let callbacks = OAuthCallbacks {
        on_auth: Box::new(|url: String| {
            println!("\nOpen this URL to continue authentication:\n  {url}\n");
            if !try_open_browser(&url) {
                println!("No local browser launcher detected. Open the URL manually.");
            }
        }),
        on_device_code: Some(Box::new(|device| {
            println!("Device verification URL: {}", device.verification_uri);
            println!(
                "bb generated this device code for you: {}",
                device.user_code
            );
            println!("Enter that code on the GitHub device page above.");
        })),
        on_manual_input: None,
        on_progress: Some(Box::new(|msg: String| {
            println!("  {msg}");
        })),
    };

    run_oauth_login(provider, callbacks).await?;
    println!("✓ Logged in to {provider}");
    if provider == "github-copilot" {
        let status = github_copilot_status();
        if let Some(authority) = status.authority {
            println!("  Authority: {authority}");
        }
        if let Some(login) = status.login {
            println!("  GitHub user: {login}");
        }
        if !status.cached_models.is_empty() {
            println!("  Refreshed Copilot models: {}", status.cached_models.len());
        }
    }
    println!("  Stored in: {}", auth_path().display());
    Ok(())
}

pub async fn handle_logout(provider: Option<&str>) -> Result<()> {
    let provider = match provider {
        Some(p) => p.to_string(),
        None => {
            let store = load_auth();
            if store.providers.is_empty() {
                println!("No providers logged in.");
                return Ok(());
            }
            println!("Logged-in providers:");
            for (i, (name, _)) in store.providers.iter().enumerate() {
                println!("  {}. {}", i + 1, name);
            }
            print!("\nSelect provider to logout: ");
            std::io::stdout().flush()?;

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let input = input.trim();

            if let Ok(num) = input.parse::<usize>() {
                let keys: Vec<_> = store.providers.keys().collect();
                if num >= 1 && num <= keys.len() {
                    keys[num - 1].clone()
                } else {
                    anyhow::bail!("Invalid selection");
                }
            } else {
                input.to_string()
            }
        }
    };

    if remove_auth(&provider)? {
        println!("✓ Logged out from {provider}");
    } else {
        println!("Provider {provider} not found in auth store.");
    }

    Ok(())
}

fn get_provider_status(name: &str) -> &'static str {
    let store = load_auth();
    if let Some(entry) = store.providers.get(name) {
        return match entry {
            AuthEntry::ApiKey { key } if !key.trim().is_empty() => "✓",
            AuthEntry::OAuth { access, .. } if !access.trim().is_empty() => "✓",
            _ => "✗",
        };
    }

    match auth_source(name) {
        Some(AuthSource::EnvVar) => "✓ (env)",
        _ => "✗",
    }
}

/// Where a credential comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    BbAuth,
    EnvVar,
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

/// Resolve which source provides auth for a provider, if any.
pub fn auth_source(provider: &str) -> Option<AuthSource> {
    let store = load_auth();
    if let Some(entry) = store.providers.get(provider) {
        let has = match entry {
            AuthEntry::ApiKey { key } => !key.trim().is_empty(),
            AuthEntry::OAuth { access, .. } => !access.trim().is_empty(),
            AuthEntry::ProviderConfig { .. } => false,
        };
        if has {
            return Some(AuthSource::BbAuth);
        }
    }
    let env_keys: &[&str] = match provider {
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "openai" | "openai-codex" => &["OPENAI_API_KEY"],
        "github-copilot" => &["GH_COPILOT_TOKEN", "GITHUB_COPILOT_TOKEN"],
        "google" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        "groq" => &["GROQ_API_KEY"],
        "xai" => &["XAI_API_KEY"],
        "openrouter" => &["OPENROUTER_API_KEY"],
        _ => &[],
    };
    for key in env_keys {
        if std::env::var(key).map(|v| !v.is_empty()).unwrap_or(false) {
            return Some(AuthSource::EnvVar);
        }
    }
    None
}

/// Resolve API key for a provider: auth.json first, then env var.
pub fn provider_has_auth(provider: &str) -> bool {
    auth_source(provider).is_some()
}

pub fn authenticated_providers() -> Vec<String> {
    let mut out = Vec::new();
    for provider in KNOWN_PROVIDERS.iter().map(|(name, _, _)| *name) {
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

pub(crate) fn authenticated_model_candidates(settings: &Settings) -> Vec<Model> {
    let available = authenticated_providers();
    if available.is_empty() {
        return Vec::new();
    }

    let mut registry = ModelRegistry::new();
    registry.load_custom_models(settings);
    add_cached_github_copilot_models(&mut registry);
    registry
        .list()
        .iter()
        .filter(|model| available.iter().any(|provider| provider == &model.provider))
        .cloned()
        .collect()
}

fn resolve_available_model_for_provider(
    settings: &Settings,
    provider: &str,
    requested_model: Option<&str>,
) -> Option<String> {
    let provider = normalize_provider_for_model_selection(provider);
    let candidates = authenticated_model_candidates(settings);
    if !candidates.iter().any(|model| model.provider == provider) {
        return None;
    }

    if let Some(requested_model) = requested_model
        && let Some(model) = candidates.iter().find(|model| {
            model.provider == provider
                && (model.id.eq_ignore_ascii_case(requested_model)
                    || model.name.eq_ignore_ascii_case(requested_model))
        })
    {
        return Some(model.id.clone());
    }

    if let Some(preferred) = preferred_model_for_provider(&provider)
        && let Some(model) = candidates.iter().find(|model| {
            model.provider == provider
                && (model.id.eq_ignore_ascii_case(&preferred)
                    || model.name.eq_ignore_ascii_case(&preferred))
        })
    {
        return Some(model.id.clone());
    }

    candidates
        .into_iter()
        .find(|model| model.provider == provider)
        .map(|model| model.id)
}

pub(crate) fn available_model_for_provider(
    settings: &Settings,
    provider: &str,
    requested_model: Option<&str>,
) -> Option<String> {
    resolve_available_model_for_provider(settings, provider, requested_model)
}

pub(crate) fn preferred_available_model_for_provider(
    settings: &Settings,
    provider: &str,
) -> Option<String> {
    available_model_for_provider(settings, provider, None)
}

pub(crate) fn preferred_model_for_provider(provider: &str) -> Option<String> {
    match provider {
        "anthropic" => Some("claude-opus-4-6".to_string()),
        "openai" | "openai-codex" => Some("gpt-5.4".to_string()),
        "google" => Some("gemini-3.1-pro".to_string()),
        "github-copilot" => {
            let cached = github_copilot_cached_models();
            cached
                .iter()
                .find(|id| id.contains("opus-4-6"))
                .cloned()
                .or_else(|| cached.iter().find(|id| id.contains("opus")).cloned())
                .or_else(|| Some("claude-opus-4-6".to_string()))
        }
        _ => None,
    }
}

pub(crate) fn preferred_startup_provider_and_model(
    settings: &bb_core::settings::Settings,
) -> Option<(String, String)> {
    if let Some(provider) = load_auth().last_provider {
        let normalized = normalize_provider_for_model_selection(&provider);
        let requested_model = if settings.default_provider.as_deref() == Some(provider.as_str())
            || settings.default_provider.as_deref() == Some(normalized.as_str())
        {
            settings.default_model.as_deref()
        } else {
            None
        };
        if let Some(model) =
            resolve_available_model_for_provider(settings, &normalized, requested_model)
        {
            return Some((normalized, model));
        }
    }

    if let Some(provider) = settings.default_provider.as_deref() {
        let normalized = normalize_provider_for_model_selection(provider);
        if let Some(model) = resolve_available_model_for_provider(
            settings,
            &normalized,
            settings.default_model.as_deref(),
        ) {
            return Some((normalized, model));
        }
    }

    for provider in authenticated_providers() {
        if let Some(model) = preferred_available_model_for_provider(settings, &provider) {
            return Some((provider, model));
        }
    }

    None
}

/// Save OAuth credentials for a provider into auth.json.
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

pub fn resolve_api_key(provider: &str) -> Option<String> {
    if provider == "github-copilot" {
        return resolve_github_copilot_api_key();
    }

    // Determine the list of auth-store keys to probe.  "openai" models should
    // also pick up "openai-codex" OAuth tokens, and vice-versa.
    let store_keys: &[&str] = match provider {
        "openai" => &["openai", "openai-codex"],
        "openai-codex" => &["openai-codex", "openai"],
        _ => &[provider],
    };

    // 1. Check BB's own auth.json (try each alias in order).
    let store = load_auth();
    for &key_name in store_keys {
        if let Some(entry) = store.providers.get(key_name) {
            match entry {
                AuthEntry::ApiKey { key } => return Some(key.clone()),
                AuthEntry::OAuth {
                    access,
                    refresh,
                    expires,
                    ..
                } => {
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    // If token is still valid (with 60s buffer), return it.
                    if *expires > now_ms + 60_000 {
                        return Some(access.clone());
                    }
                    // Try to auto-refresh.
                    if !refresh.is_empty()
                        && let Some(creds) = try_refresh_sync(key_name, refresh)
                    {
                        return Some(creds);
                    }
                    // Return stale token as last resort (server will reject).
                    return Some(access.clone());
                }
                AuthEntry::ProviderConfig { .. } => {}
            }
        }
    }

    // 2. Fall back to env var
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
            return Some(val);
        }
    }

    None
}

fn resolve_github_copilot_api_key() -> Option<String> {
    for key in ["GH_COPILOT_TOKEN", "GITHUB_COPILOT_TOKEN"] {
        if let Ok(val) = std::env::var(key)
            && !val.trim().is_empty()
        {
            return Some(val);
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
    let now_ms = chrono::Utc::now().timestamp_millis();

    if let Some(token) = extra.get("copilot_token").and_then(|value| value.as_str())
        && let Some(expires_at) = extra
            .get("copilot_expires_at")
            .and_then(|value| value.as_i64())
        && expires_at > now_ms + 300_000
        && !token.trim().is_empty()
    {
        return Some(token.to_string());
    }

    if expires <= now_ms + 60_000
        && !refresh.trim().is_empty()
        && let Some(creds) = try_refresh_sync("github-copilot", &refresh)
    {
        return Some(creds);
    }

    if access.trim().is_empty() {
        return None;
    }

    let refreshed = refresh_github_copilot_runtime_sync(&authority, &access)?;
    let mut extra = extra;
    merge_github_copilot_runtime_extra(&mut extra, &authority, &refreshed);
    let _ = save_oauth_state("github-copilot", access, refresh, expires, extra);
    Some(refreshed.copilot_token)
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

/// Best-effort synchronous token refresh.
///
/// Tries to enter the tokio runtime; if we're already inside one we
/// spawn a blocking thread with its own single-threaded runtime.
fn try_refresh_sync(provider: &str, refresh_token: &str) -> Option<String> {
    let rt = match tokio::runtime::Handle::try_current() {
        Ok(_handle) => {
            // We're inside a runtime – run on a blocking thread.
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

    // Persist the refreshed credentials.
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
