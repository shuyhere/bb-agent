mod anthropic;
mod github_copilot;
mod google;
mod groq;
mod openai;
mod openrouter;

use super::types::{ApiType, CostConfig, Model, ModelInput};

pub(crate) fn builtin_models() -> Vec<Model> {
    let mut models = Vec::new();
    models.extend(anthropic::builtin_models());
    models.extend(openai::builtin_models());
    models.extend(github_copilot::builtin_models());
    models.extend(google::builtin_models());
    models.extend(groq::builtin_models());
    models.extend(openrouter::builtin_models());
    models
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ReasoningCapability {
    Supported,
    Unsupported,
}

impl ReasoningCapability {
    fn supports_reasoning(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// Runtime defaults paired with a builtin model definition.
///
/// Keep this helper focused on deployment/runtime details that are shared across a provider's
/// builtin models, rather than threading raw booleans and URLs through every `model(...)` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RuntimeInfo {
    reasoning: ReasoningCapability,
    base_url: &'static str,
}

pub(super) fn runtime(reasoning: ReasoningCapability, base_url: &'static str) -> RuntimeInfo {
    RuntimeInfo {
        reasoning,
        base_url,
    }
}

/// Build a multimodal builtin model entry. The runtime helper is the single source of truth for
/// whether the model supports reasoning controls and which default base URL should be advertised.
pub(super) fn model(
    id: &str,
    name: &str,
    provider: &str,
    api: ApiType,
    limits: (u64, u64),
    runtime: RuntimeInfo,
    cost: CostConfig,
) -> Model {
    Model {
        id: id.into(),
        name: name.into(),
        provider: provider.into(),
        api,
        context_window: limits.0,
        max_tokens: limits.1,
        reasoning: runtime.reasoning.supports_reasoning(),
        input: vec![ModelInput::Text, ModelInput::Image],
        base_url: Some(runtime.base_url.into()),
        cost,
    }
}

/// Build a text-only builtin model entry.
pub(super) fn text_model(
    id: &str,
    name: &str,
    provider: &str,
    api: ApiType,
    limits: (u64, u64),
    runtime: RuntimeInfo,
    cost: CostConfig,
) -> Model {
    Model {
        id: id.into(),
        name: name.into(),
        provider: provider.into(),
        api,
        context_window: limits.0,
        max_tokens: limits.1,
        reasoning: runtime.reasoning.supports_reasoning(),
        input: vec![ModelInput::Text],
        base_url: Some(runtime.base_url.into()),
        cost,
    }
}

pub(super) fn cost(input: f64, output: f64, cache_read: f64, cache_write: f64) -> CostConfig {
    CostConfig {
        input,
        output,
        cache_read,
        cache_write,
    }
}

pub(super) fn simple_cost(input: f64, output: f64) -> CostConfig {
    CostConfig {
        input,
        output,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multimodal_runtime_helper_sets_reasoning_and_base_url() {
        let model = model(
            "demo",
            "Demo",
            "openai",
            ApiType::OpenaiCompletions,
            (128_000, 16_384),
            runtime(ReasoningCapability::Supported, "https://api.example.com"),
            simple_cost(1.0, 2.0),
        );

        assert!(model.reasoning);
        assert_eq!(model.base_url.as_deref(), Some("https://api.example.com"));
        assert_eq!(model.input, vec![ModelInput::Text, ModelInput::Image]);
    }

    #[test]
    fn text_model_helper_marks_text_only_inputs() {
        let model = text_model(
            "demo-text",
            "Demo Text",
            "groq",
            ApiType::OpenaiCompletions,
            (128_000, 16_384),
            runtime(ReasoningCapability::Unsupported, "https://api.example.com"),
            simple_cost(0.5, 1.5),
        );

        assert!(!model.reasoning);
        assert_eq!(model.base_url.as_deref(), Some("https://api.example.com"));
        assert_eq!(model.input, vec![ModelInput::Text]);
    }
}
