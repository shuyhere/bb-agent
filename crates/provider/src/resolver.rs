use crate::registry::{Model, ModelRegistry};

/// Parse a model argument. Supports:
///   "gpt-4o"                  → (None, "gpt-4o", None)
///   "openai/gpt-4o"           → (Some("openai"), "gpt-4o", None)
///   "sonnet:high"             → (None, "sonnet", Some("high"))
///   "anthropic/sonnet:high"   → (Some("anthropic"), "sonnet", Some("high"))
pub fn parse_model_arg(input: &str) -> (Option<String>, String, Option<String>) {
    let input = input.trim();

    // Split thinking level (must be a known level after the last ':')
    let (model_part, thinking) = if let Some(pos) = input.rfind(':') {
        let level = &input[pos + 1..];
        let valid = ["off", "low", "medium", "high", "minimal", "xhigh"];
        if valid.contains(&level) {
            (&input[..pos], Some(level.to_string()))
        } else {
            (input, None)
        }
    } else {
        (input, None)
    };

    // Split provider prefix (first '/')
    if let Some(pos) = model_part.find('/') {
        let provider = &model_part[..pos];
        let model_id = &model_part[pos + 1..];
        (Some(provider.to_string()), model_id.to_string(), thinking)
    } else {
        (None, model_part.to_string(), thinking)
    }
}

/// Fuzzy-match a model pattern against the registry.
/// Returns the best matching model or None.
pub fn fuzzy_find_model(
    registry: &ModelRegistry,
    pattern: &str,
    provider: Option<&str>,
) -> Option<Model> {
    let models = registry.list();
    let pattern_lower = pattern.to_lowercase();

    let mut best: Option<(&Model, u32)> = None;

    for model in models {
        // Filter by provider if specified
        if let Some(prov) = provider
            && model.provider != prov
        {
            continue;
        }

        let score = fuzzy_score(&pattern_lower, &model.id.to_lowercase());
        // Also try matching against display name
        let name_score = fuzzy_score(&pattern_lower, &model.name.to_lowercase());
        let final_score = score.max(name_score);

        if final_score > 0 {
            match &best {
                Some((_, best_score)) => {
                    if final_score > *best_score {
                        best = Some((model, final_score));
                    }
                }
                None => {
                    best = Some((model, final_score));
                }
            }
        }
    }

    best.map(|(m, _)| m.clone())
}

/// Simple fuzzy matching score.
/// Returns 0 if no match, higher = better match.
///
/// Scoring:
/// - Exact match: 10000
/// - Substring match: 5000 + bonus for shorter candidates
/// - Fuzzy (chars in order): 1000 + bonus for shorter candidates + word boundary bonus
pub fn fuzzy_score(pattern: &str, text: &str) -> u32 {
    if pattern.is_empty() {
        return 0;
    }

    let pattern = &pattern.to_lowercase();
    let text = &text.to_lowercase();

    // Exact match
    if pattern == text {
        return 10000;
    }

    // Substring match
    if text.contains(pattern.as_str()) {
        // Prefer shorter candidates (less noise)
        let len_bonus = 1000u32.saturating_sub(text.len() as u32 * 10);
        // Bonus if pattern matches at a word boundary
        let boundary_bonus = if text.starts_with(pattern.as_str())
            || text.contains(&format!("-{}", pattern))
            || text.contains(&format!("_{}", pattern))
        {
            500
        } else {
            0
        };
        return 5000 + len_bonus + boundary_bonus;
    }

    // Fuzzy: all pattern chars appear in order
    let mut text_chars = text.chars().peekable();
    let mut matched = 0u32;
    let mut boundary_matches = 0u32;
    let mut prev_was_boundary = true; // start of string is a boundary

    for pc in pattern.chars() {
        let mut found = false;
        for tc in text_chars.by_ref() {
            if tc == pc {
                matched += 1;
                if prev_was_boundary {
                    boundary_matches += 1;
                }
                prev_was_boundary = false;
                found = true;
                break;
            }
            prev_was_boundary = tc == '-' || tc == '_' || tc == '.' || tc == ' ';
        }
        if !found {
            return 0; // Pattern char not found
        }
    }

    // All chars matched
    let len_bonus = 500u32.saturating_sub(text.len() as u32 * 5);
    let boundary_bonus = boundary_matches * 100;
    1000 + len_bonus + boundary_bonus + matched * 10
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ModelRegistry;
    use bb_core::settings::{ModelOverride, Settings};

    #[test]
    fn test_parse_model_arg_simple() {
        let (prov, model, thinking) = parse_model_arg("gpt-4o");
        assert_eq!(prov, None);
        assert_eq!(model, "gpt-4o");
        assert_eq!(thinking, None);
    }

    #[test]
    fn test_parse_model_arg_with_provider() {
        let (prov, model, thinking) = parse_model_arg("openai/gpt-4o");
        assert_eq!(prov, Some("openai".into()));
        assert_eq!(model, "gpt-4o");
        assert_eq!(thinking, None);
    }

    #[test]
    fn test_parse_model_arg_with_thinking() {
        let (prov, model, thinking) = parse_model_arg("sonnet:high");
        assert_eq!(prov, None);
        assert_eq!(model, "sonnet");
        assert_eq!(thinking, Some("high".into()));
    }

    #[test]
    fn test_parse_model_arg_full() {
        let (prov, model, thinking) = parse_model_arg("anthropic/sonnet:high");
        assert_eq!(prov, Some("anthropic".into()));
        assert_eq!(model, "sonnet");
        assert_eq!(thinking, Some("high".into()));
    }

    #[test]
    fn test_parse_model_arg_colon_not_thinking() {
        // Colons that aren't valid thinking levels should be kept in the model id
        let (prov, model, thinking) = parse_model_arg("my-model:v2");
        assert_eq!(prov, None);
        assert_eq!(model, "my-model:v2");
        assert_eq!(thinking, None);
    }

    #[test]
    fn test_fuzzy_score_exact() {
        assert_eq!(fuzzy_score("gpt-4o", "gpt-4o"), 10000);
    }

    #[test]
    fn test_fuzzy_score_substring() {
        let score = fuzzy_score("sonnet", "claude-sonnet-4-20250514");
        assert!(score >= 5000, "substring should score >= 5000, got {score}");
    }

    #[test]
    fn test_fuzzy_score_no_match() {
        assert_eq!(fuzzy_score("xyz", "gpt-4o"), 0);
    }

    #[test]
    fn test_fuzzy_score_fuzzy_chars() {
        let score = fuzzy_score("gpt4o", "gpt-4o");
        assert!(score > 0, "fuzzy chars-in-order should match");
    }

    #[test]
    fn test_fuzzy_score_prefers_shorter() {
        let short = fuzzy_score("sonnet", "claude-sonnet-4");
        let long = fuzzy_score("sonnet", "claude-sonnet-4-20250514");
        assert!(
            short > long,
            "shorter candidate should score higher: {short} vs {long}"
        );
    }

    #[test]
    fn test_fuzzy_find_model() {
        let registry = ModelRegistry::new();

        let found = fuzzy_find_model(&registry, "sonnet", Some("anthropic"));
        assert!(found.is_some());
        let m = found.unwrap();
        assert!(
            m.id.contains("sonnet"),
            "should find sonnet model, got {}",
            m.id
        );
        assert_eq!(m.provider, "anthropic");
    }

    #[test]
    fn test_fuzzy_find_model_no_provider() {
        let registry = ModelRegistry::new();

        let found = fuzzy_find_model(&registry, "gpt-4o", None);
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "gpt-4o");
    }

    #[test]
    fn test_fuzzy_find_model_no_match() {
        let registry = ModelRegistry::new();
        let found = fuzzy_find_model(&registry, "nonexistent-xyz", None);
        assert!(found.is_none());
    }

    #[test]
    fn test_load_custom_models() {
        let settings = Settings {
            models: Some(vec![ModelOverride {
                id: "my-custom-llm".into(),
                name: Some("My Custom LLM".into()),
                provider: "local".into(),
                api: Some("openai-completions".into()),
                base_url: Some("http://localhost:8080".into()),
                context_window: Some(32000),
                max_tokens: Some(4096),
                reasoning: Some(false),
            }]),
            ..Default::default()
        };

        let mut registry = ModelRegistry::new();
        let before = registry.list().len();
        registry.load_custom_models(&settings);
        assert_eq!(registry.list().len(), before + 1);

        let found = registry.find("local", "my-custom-llm");
        assert!(found.is_some());
        let m = found.unwrap();
        assert_eq!(m.name, "My Custom LLM");
        assert_eq!(m.context_window, 32000);
        assert_eq!(m.base_url.as_deref(), Some("http://localhost:8080"));
    }

    #[test]
    fn test_fuzzy_find_via_registry() {
        let registry = ModelRegistry::new();
        let found = registry.find_fuzzy("sonnet", Some("anthropic"));
        assert!(found.is_some());
        assert!(found.unwrap().id.contains("sonnet"));

        let found2 = registry.find_fuzzy("gpt-4o", None);
        assert!(found2.is_some());
        assert_eq!(found2.unwrap().id, "gpt-4o");
    }
}
