use super::*;

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

fn preferred_model_for_provider(provider: &str) -> Option<String> {
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
    // If the user explicitly configured a default provider/model, honor that
    // before any heuristic startup preference.
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

    // Otherwise prefer OpenAI first when it is authenticated, so the app's
    // startup default matches the global fallback default model (gpt-5.4).
    if let Some(model) = resolve_available_model_for_provider(settings, "openai", Some("gpt-5.4")) {
        return Some(("openai".to_string(), model));
    }
    if let Some(model) =
        resolve_available_model_for_provider(settings, "openai-codex", Some("gpt-5.4"))
    {
        return Some(("openai-codex".to_string(), model));
    }

    // Next prefer the most recently-used provider, if still authenticated.
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

    for provider in authenticated_providers() {
        if let Some(model) = preferred_available_model_for_provider(settings, &provider) {
            return Some((provider, model));
        }
    }

    None
}
