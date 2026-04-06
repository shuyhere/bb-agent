use anyhow::Result;
use bb_core::config;
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
}

const KNOWN_PROVIDERS: &[(&str, &str, &str)] = &[
    (
        "anthropic",
        "ANTHROPIC_API_KEY",
        "https://console.anthropic.com/settings/keys",
    ),
    ("openai-codex", "", "https://chatgpt.com/"),
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
const OAUTH_PROVIDERS: &[&str] = &["anthropic", "openai-codex"];

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
    save_auth(&store)
}

pub(crate) async fn run_oauth_login(
    provider: &str,
    callbacks: crate::oauth::OAuthCallbacks,
) -> Result<()> {
    use crate::oauth;

    let creds = match provider {
        "anthropic" => oauth::login_anthropic(callbacks).await?,
        "openai-codex" => oauth::login_openai_codex(callbacks).await?,
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
        on_manual_input: None,
        on_progress: Some(Box::new(|msg: String| {
            println!("  {msg}");
        })),
    };

    run_oauth_login(provider, callbacks).await?;
    println!("✓ Logged in to {provider}");
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
    if store.providers.contains_key(name) {
        "✓"
    } else {
        // Check env var
        let env_var = KNOWN_PROVIDERS
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, e, _)| *e)
            .unwrap_or("");
        if !env_var.is_empty() {
            if std::env::var(env_var)
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            {
                "✓ (env)"
            } else {
                "✗"
            }
        } else {
            "✗"
        }
    }
}

/// Where a credential comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    BbAuth,
    EnvVar,
}

/// Resolve which source provides auth for a provider, if any.
pub fn auth_source(provider: &str) -> Option<AuthSource> {
    let store = load_auth();
    if let Some(entry) = store.providers.get(provider) {
        let has = match entry {
            AuthEntry::ApiKey { key } => !key.trim().is_empty(),
            AuthEntry::OAuth { access, .. } => !access.trim().is_empty(),
        };
        if has {
            return Some(AuthSource::BbAuth);
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
        if std::env::var(key).map(|v| !v.is_empty()).unwrap_or(false) {
            return Some(AuthSource::EnvVar);
        }
    }
    None
}

/// Resolve API key for a provider: auth.json first, then env var.
pub fn provider_has_auth(provider: &str) -> bool {
    resolve_api_key(provider)
        .map(|key| !key.trim().is_empty())
        .unwrap_or(false)
}

pub fn authenticated_providers() -> Vec<String> {
    let mut out: Vec<String> = KNOWN_PROVIDERS
        .iter()
        .map(|(name, _, _)| *name)
        .filter(|provider| provider_has_auth(provider))
        .map(str::to_string)
        .collect();

    // When openai-codex is authenticated, also expose "openai" so that
    // models registered under the "openai" provider are selectable.
    if out.iter().any(|p| p == "openai-codex") && !out.iter().any(|p| p == "openai") {
        out.push("openai".to_string());
    }
    out
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
    save_auth(&store)
}

pub fn resolve_api_key(provider: &str) -> Option<String> {
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
        _ => &["OPENAI_API_KEY"],
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
        _ => return None,
    };

    // Persist the refreshed credentials.
    let _ = save_oauth_credentials(provider, &creds);
    Some(creds.access)
}
