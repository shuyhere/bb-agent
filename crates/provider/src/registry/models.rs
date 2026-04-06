mod anthropic;
mod google;
mod groq;
mod openai;
mod openrouter;

use super::types::{ApiType, CostConfig, Model};

pub(crate) fn builtin_models() -> Vec<Model> {
    let mut models = Vec::new();
    models.extend(anthropic::builtin_models());
    models.extend(openai::builtin_models());
    models.extend(google::builtin_models());
    models.extend(groq::builtin_models());
    models.extend(openrouter::builtin_models());
    models
}

pub(super) struct RuntimeInfo {
    reasoning: bool,
    base_url: &'static str,
}

pub(super) fn runtime(reasoning: bool, base_url: &'static str) -> RuntimeInfo {
    RuntimeInfo {
        reasoning,
        base_url,
    }
}

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
        reasoning: runtime.reasoning,
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
