mod resolver;
mod store;

use anyhow::Result;
use bb_core::config;
use bb_core::settings::Settings;
use bb_provider::registry::{Model, ModelRegistry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use crate::oauth::OAuthCredentials;

use resolver::AuthSource;
use store::{AuthEntry, load_auth};

pub(crate) use resolver::{
    add_cached_github_copilot_models, auth_source, auth_source_label,
    authenticated_model_candidates, available_model_for_provider,
    preferred_available_model_for_provider, preferred_startup_provider_and_model,
    provider_auth_status_summary, resolve_api_key,
};
pub(crate) use store::{
    auth_path, configured_providers, github_copilot_api_base_url, github_copilot_cached_models,
    github_copilot_domain, github_copilot_runtime_headers, github_copilot_status,
    normalize_github_domain, remove_auth, save_api_key, save_github_copilot_config,
};

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

    resolver::save_oauth_credentials(provider, &creds)
}

pub async fn handle_login(provider: Option<&str>) -> Result<()> {
    let provider = match provider {
        Some(p) => p.to_string(),
        None => {
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

    if OAUTH_PROVIDERS.contains(&provider.as_str()) {
        return handle_oauth_login_cli(&provider).await;
    }

    let (_, env_var, url) = KNOWN_PROVIDERS
        .iter()
        .find(|(name, _, _)| *name == provider)
        .copied()
        .unwrap_or((&provider, "API_KEY", ""));

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
