use super::{model, runtime, simple_cost};
use crate::registry::{ApiType, Model};

pub(super) fn builtin_models() -> Vec<Model> {
    vec![
        model(
            "gemini-2.5-flash",
            "Gemini 2.5 Flash",
            "google",
            ApiType::GoogleGenerative,
            (1_048_576, 65_536),
            runtime(true, "https://generativelanguage.googleapis.com"),
            simple_cost(0.15, 0.6),
        ),
        model(
            "gemini-2.5-pro",
            "Gemini 2.5 Pro",
            "google",
            ApiType::GoogleGenerative,
            (1_048_576, 65_536),
            runtime(true, "https://generativelanguage.googleapis.com"),
            simple_cost(1.25, 10.0),
        ),
        model(
            "gemini-2.5-flash-lite",
            "Gemini 2.5 Flash Lite",
            "google",
            ApiType::GoogleGenerative,
            (1_048_576, 65_536),
            runtime(false, "https://generativelanguage.googleapis.com"),
            simple_cost(0.075, 0.3),
        ),
    ]
}
