use super::{ReasoningCapability, runtime, simple_cost, text_model};
use crate::registry::{ApiType, Model};

pub(super) fn builtin_models() -> Vec<Model> {
    vec![
        text_model(
            "llama-3.3-70b-versatile",
            "Llama 3.3 70B",
            "groq",
            ApiType::OpenaiCompletions,
            (128_000, 32_768),
            runtime(
                ReasoningCapability::Unsupported,
                "https://api.groq.com/openai/v1",
            ),
            simple_cost(0.59, 0.79),
        ),
        text_model(
            "llama-3.1-8b-instant",
            "Llama 3.1 8B Instant",
            "groq",
            ApiType::OpenaiCompletions,
            (131_072, 8_192),
            runtime(
                ReasoningCapability::Unsupported,
                "https://api.groq.com/openai/v1",
            ),
            simple_cost(0.05, 0.08),
        ),
        text_model(
            "mixtral-8x7b-32768",
            "Mixtral 8x7B",
            "groq",
            ApiType::OpenaiCompletions,
            (32_768, 32_768),
            runtime(
                ReasoningCapability::Unsupported,
                "https://api.groq.com/openai/v1",
            ),
            simple_cost(0.24, 0.24),
        ),
    ]
}
