use super::{ReasoningCapability, model, runtime, simple_cost};
use crate::registry::{ApiType, Model};

pub(super) fn builtin_models() -> Vec<Model> {
    vec![
        model(
            "claude-opus-4-6",
            "Claude Opus 4.6 (Copilot)",
            "github-copilot",
            ApiType::OpenaiCompletions,
            (200_000, 16_384),
            runtime(
                ReasoningCapability::Supported,
                "https://api.githubcopilot.com",
            ),
            simple_cost(0.0, 0.0),
        ),
        model(
            "gpt-4o",
            "GPT-4o (Copilot)",
            "github-copilot",
            ApiType::OpenaiCompletions,
            (128_000, 16_384),
            runtime(
                ReasoningCapability::Unsupported,
                "https://api.githubcopilot.com",
            ),
            simple_cost(0.0, 0.0),
        ),
        model(
            "claude-sonnet-4",
            "Claude Sonnet 4 (Copilot)",
            "github-copilot",
            ApiType::OpenaiCompletions,
            (200_000, 16_384),
            runtime(
                ReasoningCapability::Supported,
                "https://api.githubcopilot.com",
            ),
            simple_cost(0.0, 0.0),
        ),
        model(
            "o3-mini",
            "o3-mini (Copilot)",
            "github-copilot",
            ApiType::OpenaiCompletions,
            (200_000, 100_000),
            runtime(
                ReasoningCapability::Supported,
                "https://api.githubcopilot.com",
            ),
            simple_cost(0.0, 0.0),
        ),
    ]
}
