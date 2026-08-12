use super::{ReasoningCapability, model, runtime, simple_cost, text_model};
use crate::registry::{ApiType, Model};

const XAI_BASE_URL: &str = "https://api.x.ai/v1";

pub(super) fn builtin_models() -> Vec<Model> {
    vec![
        // Grok Build / coding agent models reject OpenAI-style reasoning_effort.
        text_model(
            "grok-build-0.1",
            "Grok Build 0.1",
            "xai",
            ApiType::OpenaiCompletions,
            (256_000, 64_000),
            runtime(ReasoningCapability::Unsupported, XAI_BASE_URL),
            simple_cost(0.0, 0.0),
        ),
        model(
            "grok-4.5",
            "Grok 4.5",
            "xai",
            ApiType::OpenaiCompletions,
            (256_000, 64_000),
            runtime(ReasoningCapability::Supported, XAI_BASE_URL),
            simple_cost(0.0, 0.0),
        ),
        model(
            "grok-4.3",
            "Grok 4.3",
            "xai",
            ApiType::OpenaiCompletions,
            (256_000, 64_000),
            runtime(ReasoningCapability::Supported, XAI_BASE_URL),
            simple_cost(0.0, 0.0),
        ),
        // Composer-fast chat models do not accept reasoning_effort.
        text_model(
            "grok-composer-2.5-fast",
            "Grok Composer 2.5 Fast",
            "xai",
            ApiType::OpenaiCompletions,
            (256_000, 64_000),
            runtime(ReasoningCapability::Unsupported, XAI_BASE_URL),
            simple_cost(0.0, 0.0),
        ),
        text_model(
            "grok-4.20-0309-reasoning",
            "Grok 4.20 Reasoning",
            "xai",
            ApiType::OpenaiCompletions,
            (256_000, 64_000),
            runtime(ReasoningCapability::Supported, XAI_BASE_URL),
            simple_cost(0.0, 0.0),
        ),
        text_model(
            "grok-4.20-0309-non-reasoning",
            "Grok 4.20 Non-Reasoning",
            "xai",
            ApiType::OpenaiCompletions,
            (256_000, 64_000),
            runtime(ReasoningCapability::Unsupported, XAI_BASE_URL),
            simple_cost(0.0, 0.0),
        ),
        text_model(
            "grok-4.20-multi-agent-0309",
            "Grok 4.20 Multi-Agent",
            "xai",
            ApiType::OpenaiCompletions,
            (256_000, 64_000),
            runtime(ReasoningCapability::Supported, XAI_BASE_URL),
            simple_cost(0.0, 0.0),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::builtin_models;

    #[test]
    fn xai_models_use_api_x_ai_base_url() {
        let models = builtin_models();
        assert!(!models.is_empty());
        for model in models {
            assert_eq!(model.provider, "xai");
            assert_eq!(
                model.base_url.as_deref(),
                Some("https://api.x.ai/v1"),
                "model {} missing xAI base URL",
                model.id
            );
        }
    }

    #[test]
    fn xai_models_include_oauth_defaults() {
        let ids: Vec<_> = builtin_models().into_iter().map(|m| m.id).collect();
        assert!(ids.contains(&"grok-build-0.1".to_string()));
        assert!(ids.contains(&"grok-4.5".to_string()));
    }

    #[test]
    fn grok_build_and_composer_do_not_advertise_reasoning() {
        let models = builtin_models();
        let build = models
            .iter()
            .find(|m| m.id == "grok-build-0.1")
            .expect("grok-build-0.1");
        let composer = models
            .iter()
            .find(|m| m.id == "grok-composer-2.5-fast")
            .expect("composer");
        assert!(
            !build.reasoning,
            "grok-build-0.1 must not advertise reasoning (API rejects reasoningEffort)"
        );
        assert!(!composer.reasoning);
        let reasoning = models
            .iter()
            .find(|m| m.id == "grok-4.20-0309-reasoning")
            .expect("reasoning model");
        assert!(reasoning.reasoning);
    }
}
