use super::{ReasoningCapability, model, runtime, simple_cost};
use crate::registry::{ApiType, Model};

pub(super) fn builtin_models() -> Vec<Model> {
    vec![
        model(
            "anthropic/claude-sonnet-4",
            "Claude Sonnet 4 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (200_000, 64_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(3.0, 15.0),
        ),
        model(
            "anthropic/claude-opus-4",
            "Claude Opus 4 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (200_000, 32_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(15.0, 75.0),
        ),
        model(
            "openai/gpt-5",
            "GPT-5 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (256_000, 64_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(2.0, 8.0),
        ),
    ]
}
