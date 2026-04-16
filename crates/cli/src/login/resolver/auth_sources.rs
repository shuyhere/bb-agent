use super::*;

fn auth_methods_for_provider(provider: &str) -> (bool, bool) {
    let store = load_auth();
    let mut has_oauth =
        stored_auth_entry_for_method(&store, provider, ProviderAuthMethod::OAuth).is_some();
    let mut has_api_key =
        stored_auth_entry_for_method(&store, provider, ProviderAuthMethod::ApiKey).is_some();

    match normalize_provider_for_model_selection(provider).as_str() {
        "anthropic" => {
            if std::env::var("ANTHROPIC_API_KEY")
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            {
                has_api_key = true;
            }
        }
        "openai" | "openai-codex" => {
            if std::env::var("OPENAI_API_KEY")
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            {
                has_api_key = true;
            }
        }
        "github-copilot" => {
            for key in ["GH_COPILOT_TOKEN", "GITHUB_COPILOT_TOKEN"] {
                if std::env::var(key)
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
                {
                    has_oauth = true;
                }
            }
        }
        other => {
            let env_keys: &[&str] = match other {
                "google" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
                "groq" => &["GROQ_API_KEY"],
                "xai" => &["XAI_API_KEY"],
                "openrouter" => &["OPENROUTER_API_KEY"],
                _ => &[],
            };
            for key in env_keys {
                if std::env::var(key)
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
                {
                    has_api_key = true;
                }
            }
        }
    }

    (has_oauth, has_api_key)
}

pub(crate) fn provider_auth_status_summary(provider: &str) -> String {
    let (has_oauth, has_api_key) = auth_methods_for_provider(provider);
    let active = active_auth_method(provider);
    let base = if has_oauth && has_api_key {
        "[OAuth + API key configured]".to_string()
    } else if has_oauth {
        "[OAuth configured]".to_string()
    } else if has_api_key {
        "[API key configured]".to_string()
    } else {
        "[not authenticated]".to_string()
    };

    match active {
        Some(method) if has_oauth && has_api_key => {
            format!("{base} • active: {}", method.label())
        }
        _ => base,
    }
}

pub(crate) fn add_cached_github_copilot_models(registry: &mut ModelRegistry) {
    for model_id in github_copilot_cached_models() {
        if registry.find("github-copilot", &model_id).is_none() {
            registry.add(Model {
                id: model_id.clone(),
                name: model_id.clone(),
                provider: "github-copilot".to_string(),
                api: bb_provider::registry::ApiType::OpenaiCompletions,
                context_window: 128_000,
                max_tokens: 16_384,
                reasoning: true,
                input: vec![bb_provider::registry::ModelInput::Text],
                base_url: Some(github_copilot_api_base_url()),
                cost: Default::default(),
            });
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthSource {
    BbAuth,
    EnvVar,
}

impl AuthSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            AuthSource::BbAuth => "bb auth.json",
            AuthSource::EnvVar => "environment",
        }
    }
}

pub fn auth_source(provider: &str) -> Option<AuthSource> {
    let store = load_auth();
    if !stored_auth_methods_for_store(&store, provider).is_empty() {
        return Some(AuthSource::BbAuth);
    }
    let env_keys: &[&str] = match normalize_provider_for_model_selection(provider).as_str() {
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

pub fn provider_has_auth(provider: &str) -> bool {
    auth_source(provider).is_some()
}

pub fn authenticated_providers() -> Vec<String> {
    let mut out = Vec::new();
    for provider in known_providers().iter().map(|(name, _, _)| *name) {
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
