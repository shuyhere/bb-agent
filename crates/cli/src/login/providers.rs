use std::borrow::Cow;

use super::*;

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

pub(super) fn known_providers() -> &'static [(&'static str, &'static str, &'static str)] {
    KNOWN_PROVIDERS
}

pub(super) fn is_oauth_provider(provider: &str) -> bool {
    OAUTH_PROVIDERS.contains(&provider)
}

pub(super) fn normalize_provider_for_model_selection(provider: &str) -> String {
    match provider {
        "openai-codex" => "openai".to_string(),
        other => other.to_string(),
    }
}

/// Resolve the environment-variable hint and help URL used by both the CLI
/// and TUI login flows.
///
/// Examples:
/// - `provider_meta("google")` => (`"GOOGLE_API_KEY"`, `"https://aistudio.google.com/app/apikey"`)
/// - unknown providers fall back to (`"API_KEY"`, `""`)
pub(crate) fn provider_meta(provider: &str) -> (&str, &str) {
    KNOWN_PROVIDERS
        .iter()
        .find(|(name, _, _)| *name == provider)
        .map(|(_, env_var, url)| (*env_var, *url))
        .unwrap_or(("API_KEY", ""))
}

/// Human-readable provider label reused across login prompts, TUI menus, and
/// session status rendering.
///
/// Known providers borrow a static label; unknown providers fall back to the
/// raw provider name without allocating a new `String`.
pub(crate) fn provider_display_name(provider: &str) -> Cow<'_, str> {
    match provider {
        "anthropic" => Cow::Borrowed("Claude Pro/Max"),
        "openai-codex" => Cow::Borrowed("ChatGPT Plus/Pro (Codex)"),
        "github-copilot" => Cow::Borrowed("GitHub Copilot"),
        "openai" => Cow::Borrowed("OpenAI"),
        "google" => Cow::Borrowed("Google Gemini"),
        "groq" => Cow::Borrowed("Groq"),
        "xai" => Cow::Borrowed("xAI"),
        "openrouter" => Cow::Borrowed("OpenRouter"),
        _ => Cow::Borrowed(provider),
    }
}

/// Authentication mechanism shown in login menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderAuthMethod {
    OAuth,
    ApiKey,
}

impl ProviderAuthMethod {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::OAuth => "OAuth",
            Self::ApiKey => "API key",
        }
    }

    pub(crate) fn footer_label(self) -> &'static str {
        match self {
            Self::OAuth => "oauth",
            Self::ApiKey => "api-key",
        }
    }
}

/// Return the login method used for a provider so callers can format their own
/// UI labels without relying on stringly-typed flags.
pub(crate) fn provider_auth_method(provider: &str) -> ProviderAuthMethod {
    if is_oauth_provider(provider) {
        ProviderAuthMethod::OAuth
    } else {
        ProviderAuthMethod::ApiKey
    }
}

/// Explain the non-obvious login behavior for a provider.
///
/// This is intentionally shared between `bb login` and the TUI auth menus so
/// provider-specific caveats stay consistent in both entry points.
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

pub(super) fn get_provider_status(name: &str) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::{
        ProviderAuthMethod, is_oauth_provider, normalize_provider_for_model_selection,
        provider_api_key_variant, provider_auth_method, provider_display_name, provider_login_hint,
        provider_meta, provider_oauth_variant,
    };

    #[test]
    fn provider_meta_returns_known_and_fallback_values() {
        assert_eq!(
            provider_meta("google"),
            ("GOOGLE_API_KEY", "https://aistudio.google.com/app/apikey")
        );
        assert_eq!(provider_meta("unknown-provider"), ("API_KEY", ""));
    }

    #[test]
    fn provider_display_name_covers_known_and_unknown_providers() {
        assert_eq!(provider_display_name("github-copilot"), "GitHub Copilot");
        assert_eq!(
            provider_display_name("openai-codex"),
            "ChatGPT Plus/Pro (Codex)"
        );
        assert_eq!(provider_display_name("custom"), "custom");
    }

    #[test]
    fn oauth_and_api_key_variants_are_reported_consistently() {
        assert!(is_oauth_provider("anthropic"));
        assert!(is_oauth_provider("github-copilot"));
        assert!(!is_oauth_provider("google"));

        assert_eq!(
            provider_auth_method("openai-codex"),
            ProviderAuthMethod::OAuth
        );
        assert_eq!(
            provider_auth_method("openrouter"),
            ProviderAuthMethod::ApiKey
        );
        assert_eq!(provider_auth_method("openrouter").label(), "API key");

        assert_eq!(provider_oauth_variant("openai"), Some("openai-codex"));
        assert_eq!(provider_oauth_variant("google"), None);
        assert_eq!(provider_api_key_variant("openai-codex"), Some("openai"));
        assert_eq!(provider_api_key_variant("github-copilot"), None);
    }

    #[test]
    fn provider_login_hints_match_provider_type() {
        let oauth_hint = provider_login_hint("openai-codex");
        assert!(oauth_hint.contains("browser OAuth"));
        assert!(oauth_hint.contains("ChatGPT Plus or Pro"));

        let api_key_hint = provider_login_hint("google");
        assert!(api_key_hint.contains("GOOGLE_API_KEY"));
        assert!(api_key_hint.contains("aistudio.google.com"));

        let fallback_hint = provider_login_hint("custom");
        assert_eq!(fallback_hint, "Set API_KEY or paste an API key.");
    }

    #[test]
    fn provider_name_normalization_keeps_model_selection_aliases_stable() {
        assert_eq!(
            normalize_provider_for_model_selection("openai-codex"),
            "openai"
        );
        assert_eq!(
            normalize_provider_for_model_selection("anthropic"),
            "anthropic"
        );
    }
}
