pub fn parse_model_arg(
    provider: Option<&str>,
    model: Option<&str>,
) -> (String, String, Option<String>) {
    let default_provider = provider.unwrap_or("openai").to_string();
    let default_model = match default_provider.as_str() {
        "anthropic" => "claude-opus-4-6",
        "openai" | "openai-codex" => "gpt-5.4",
        "google" => "gemini-3.1-pro",
        "github-copilot" => "gpt-5.4",
        _ => "gpt-5.4",
    };

    let model_str = match model {
        Some(model) => model,
        None => return (default_provider, default_model.to_string(), None),
    };

    let (model_part, thinking) = if let Some(pos) = model_str.rfind(':') {
        let level = &model_str[pos + 1..];
        let valid = ["off", "low", "medium", "high", "minimal", "xhigh"];
        if valid.contains(&level) {
            (&model_str[..pos], Some(level.to_string()))
        } else {
            (model_str, None)
        }
    } else {
        (model_str, None)
    };

    if let Some(pos) = model_part.find('/') {
        let provider_name = &model_part[..pos];
        let model_id = &model_part[pos + 1..];
        (provider_name.to_string(), model_id.to_string(), thinking)
    } else {
        (default_provider, model_part.to_string(), thinking)
    }
}
