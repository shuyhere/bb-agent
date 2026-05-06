#[cfg(test)]
mod tests {
    use crate::agent_session::parse_model_arg;

    #[test]
    fn defaults_to_latest_builtin_models_for_core_providers() {
        let (provider, model, thinking) = parse_model_arg(Some("anthropic"), None);
        assert_eq!(provider, "anthropic");
        assert_eq!(model, "claude-opus-4-6");
        assert_eq!(thinking, None);

        let (provider, model, thinking) = parse_model_arg(Some("openai"), None);
        assert_eq!(provider, "openai");
        assert_eq!(model, "gpt-5.4");
        assert_eq!(thinking, None);

        let (provider, model, thinking) = parse_model_arg(Some("google"), None);
        assert_eq!(provider, "google");
        assert_eq!(model, "gemini-3.1-pro-preview");
        assert_eq!(thinking, None);

        let (provider, model, thinking) = parse_model_arg(Some("github-copilot"), None);
        assert_eq!(provider, "github-copilot");
        assert_eq!(model, "gpt-5.4");
        assert_eq!(thinking, None);
    }

    #[test]
    fn keeps_explicit_model_and_thinking_suffix() {
        let (provider, model, thinking) = parse_model_arg(Some("openai"), Some("gpt-5.4:high"));
        assert_eq!(provider, "openai");
        assert_eq!(model, "gpt-5.4");
        assert_eq!(thinking.as_deref(), Some("high"));
    }
}
