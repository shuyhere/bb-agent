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
            "anthropic/claude-sonnet-4.5",
            "Claude Sonnet 4.5 (OpenRouter)",
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
            "anthropic/claude-sonnet-4.6",
            "Claude Sonnet 4.6 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (1_000_000, 64_000),
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
            "anthropic/claude-opus-4.1",
            "Claude Opus 4.1 (OpenRouter)",
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
            "anthropic/claude-opus-4.5",
            "Claude Opus 4.5 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (200_000, 64_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(5.0, 25.0),
        ),
        model(
            "anthropic/claude-opus-4.6",
            "Claude Opus 4.6 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (1_000_000, 128_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(5.0, 25.0),
        ),
        model(
            "anthropic/claude-opus-4.6-fast",
            "Claude Opus 4.6 Fast (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (1_000_000, 128_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(5.0, 25.0),
        ),
        model(
            "anthropic/claude-opus-4.7",
            "Claude Opus 4.7 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (1_000_000, 128_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(5.0, 25.0),
        ),
        model(
            "google/gemini-2.5-flash",
            "Gemini 2.5 Flash (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (1_048_576, 65_536),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(0.15, 0.6),
        ),
        model(
            "google/gemini-2.5-pro",
            "Gemini 2.5 Pro (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (1_048_576, 65_536),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(1.25, 10.0),
        ),
        model(
            "google/gemini-3.1-pro-preview",
            "Gemini 3.1 Pro Preview (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (1_048_576, 65_536),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(1.25, 10.0),
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
        model(
            "openai/gpt-5.4",
            "GPT-5.4 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (272_000, 128_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(2.5, 15.0),
        ),
        model(
            "openai/gpt-5.5",
            "GPT-5.5 (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (272_000, 128_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(5.0, 30.0),
        ),
        model(
            "deepseek/deepseek-v4-pro",
            "DeepSeek V4 Pro (OpenRouter)",
            "openrouter",
            ApiType::OpenaiCompletions,
            (1_000_000, 384_000),
            runtime(
                ReasoningCapability::Supported,
                "https://openrouter.ai/api/v1",
            ),
            simple_cost(0.435, 0.87),
        ),
    ]
}
