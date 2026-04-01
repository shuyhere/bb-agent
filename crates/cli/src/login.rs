use anyhow::Result;
use bb_core::config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

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
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<i64>,
    },
}

const KNOWN_PROVIDERS: &[(&str, &str, &str)] = &[
    ("anthropic", "ANTHROPIC_API_KEY", "https://console.anthropic.com/settings/keys"),
    ("openai", "OPENAI_API_KEY", "https://platform.openai.com/api-keys"),
    ("google", "GOOGLE_API_KEY", "https://aistudio.google.com/app/apikey"),
    ("groq", "GROQ_API_KEY", "https://console.groq.com/keys"),
    ("xai", "XAI_API_KEY", "https://console.x.ai/"),
    ("openrouter", "OPENROUTER_API_KEY", "https://openrouter.ai/settings/keys"),
];

fn auth_path() -> PathBuf {
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
    std::fs::write(&path, content)?;
    Ok(())
}

pub async fn handle_login(provider: Option<&str>) -> Result<()> {
    let provider = match provider {
        Some(p) => p.to_string(),
        None => {
            // Show provider selector
            println!("Available providers:");
            for (i, (name, _, url)) in KNOWN_PROVIDERS.iter().enumerate() {
                let status = get_provider_status(name);
                println!("  {}. {} {} ({})", i + 1, name, status, url);
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

    let (_, env_var, url) = KNOWN_PROVIDERS
        .iter()
        .find(|(name, _, _)| *name == provider)
        .copied()
        .unwrap_or((&provider, "API_KEY", ""));

    // Check if already have env var
    if let Ok(key) = std::env::var(env_var) {
        if !key.is_empty() {
            println!("✓ {} is already set via environment variable {}", provider, env_var);
            print!("Override with manual key? [y/N]: ");
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                return Ok(());
            }
        }
    }

    if !url.is_empty() {
        println!("\nGet your API key from: {url}");
    }
    println!("(Tip: you can also set {} environment variable instead)\n", env_var);

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

    // Save to auth store
    let mut store = load_auth();
    store
        .providers
        .insert(provider.clone(), AuthEntry::ApiKey { key });
    save_auth(&store)?;

    println!("✓ API key saved for {provider}");
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

    let mut store = load_auth();
    if store.providers.remove(&provider).is_some() {
        save_auth(&store)?;
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
            if std::env::var(env_var).map(|v| !v.is_empty()).unwrap_or(false) {
                "✓ (env)"
            } else {
                "✗"
            }
        } else {
            "✗"
        }
    }
}

/// Resolve API key for a provider: auth.json first, then env var.
pub fn resolve_api_key(provider: &str) -> Option<String> {
    // Check auth.json first
    let store = load_auth();
    if let Some(entry) = store.providers.get(provider) {
        match entry {
            AuthEntry::ApiKey { key } => return Some(key.clone()),
            AuthEntry::OAuth { access_token, .. } => return Some(access_token.clone()),
        }
    }

    // Fall back to env var
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
        if let Ok(val) = std::env::var(key) {
            if !val.is_empty() {
                return Some(val);
            }
        }
    }

    None
}
