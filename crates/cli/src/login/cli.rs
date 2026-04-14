use super::*;
use std::io::Write;

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

pub(crate) async fn handle_login(provider: Option<&str>) -> Result<()> {
    let provider = match provider {
        Some(p) => p.to_string(),
        None => prompt_for_provider_login()?,
    };

    if provider == "github-copilot" {
        let domain = prompt_for_github_copilot_domain()?;
        save_github_copilot_config(&domain)?;
        println!("✓ Saved GitHub Copilot target: {domain}");
        return handle_oauth_login_cli(&provider).await;
    }

    if is_oauth_provider(provider.as_str()) {
        return handle_oauth_login_cli(&provider).await;
    }

    handle_api_key_login_cli(&provider)
}

pub(crate) async fn handle_logout(provider: Option<&str>) -> Result<()> {
    let Some(provider) = (match provider {
        Some(p) => Some(p.to_string()),
        None => prompt_for_provider_logout()?,
    }) else {
        return Ok(());
    };

    if remove_auth(&provider)? {
        println!("✓ Logged out from {provider}");
    } else {
        println!("Provider {provider} not found in auth store.");
    }

    Ok(())
}

fn prompt_for_provider_login() -> Result<String> {
    println!("Available providers:");
    for (i, (name, _, _url)) in known_providers().iter().enumerate() {
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
    let names = known_providers()
        .iter()
        .map(|(name, _, _)| *name)
        .collect::<Vec<_>>();
    resolve_provider_selection(&input, &names)
}

fn prompt_for_github_copilot_domain() -> Result<String> {
    println!("GitHub Copilot sign-in target:");
    println!("  Press Enter for github.com, or enter your GitHub Enterprise Server domain.");
    print!("Domain [github.com]: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    resolve_github_copilot_domain_input(&input)
}

fn handle_api_key_login_cli(provider: &str) -> Result<()> {
    let (env_var, url) = provider_meta(provider);

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

    save_api_key(provider, key)?;

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

fn prompt_for_provider_logout() -> Result<Option<String>> {
    let store = load_auth();
    if store.providers.is_empty() {
        println!("No providers logged in.");
        return Ok(None);
    }

    println!("Logged-in providers:");
    let provider_names = store
        .providers
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    for (i, name) in provider_names.iter().enumerate() {
        println!("  {}. {}", i + 1, name);
    }
    print!("\nSelect provider to logout: ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    resolve_provider_selection(&input, &provider_names).map(Some)
}

fn resolve_provider_selection(input: &str, provider_names: &[&str]) -> Result<String> {
    let input = input.trim();
    if let Ok(num) = input.parse::<usize>() {
        if num >= 1 && num <= provider_names.len() {
            Ok(provider_names[num - 1].to_string())
        } else {
            anyhow::bail!("Invalid selection");
        }
    } else {
        Ok(input.to_string())
    }
}

fn resolve_github_copilot_domain_input(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok("github.com".to_string())
    } else {
        normalize_github_domain(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_github_copilot_domain_input, resolve_provider_selection};

    #[test]
    fn resolve_provider_selection_accepts_numeric_choice() {
        let providers = ["anthropic", "github-copilot", "openai"];
        assert_eq!(
            resolve_provider_selection("2", &providers).unwrap(),
            "github-copilot"
        );
    }

    #[test]
    fn resolve_provider_selection_rejects_out_of_range_choice() {
        let providers = ["anthropic", "github-copilot"];
        assert!(resolve_provider_selection("3", &providers).is_err());
    }

    #[test]
    fn resolve_provider_selection_preserves_named_provider_input() {
        let providers = ["anthropic", "github-copilot"];
        assert_eq!(
            resolve_provider_selection(" openai ", &providers).unwrap(),
            "openai"
        );
    }

    #[test]
    fn resolve_github_copilot_domain_defaults_and_normalizes() {
        assert_eq!(
            resolve_github_copilot_domain_input("   ").unwrap(),
            "github.com"
        );
        assert_eq!(
            resolve_github_copilot_domain_input("https://GHE.EXAMPLE.com/login").unwrap(),
            "ghe.example.com"
        );
    }
}
