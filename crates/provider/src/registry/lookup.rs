use std::path::Path;

use bb_core::settings::Settings;

use super::models::builtin_models;
use super::types::{ApiType, CostConfig, Model};

/// Model registry holding all available models.
pub struct ModelRegistry {
    models: Vec<Model>,
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            models: builtin_models(),
        }
    }

    pub fn find(&self, provider: &str, model_id: &str) -> Option<&Model> {
        self.models
            .iter()
            .find(|m| m.provider == provider && m.id == model_id)
    }

    pub fn list(&self) -> &[Model] {
        &self.models
    }

    pub fn add(&mut self, model: Model) {
        self.models.push(model);
    }

    /// Load additional models from settings model overrides.
    pub fn load_custom_models(&mut self, settings: &Settings) {
        if let Some(models) = &settings.models {
            for mo in models {
                let api = mo
                    .api
                    .as_deref()
                    .map(|a| match a {
                        "anthropic-messages" => ApiType::AnthropicMessages,
                        "openai-responses" => ApiType::OpenaiResponses,
                        "google-generative" => ApiType::GoogleGenerative,
                        _ => ApiType::OpenaiCompletions,
                    })
                    .unwrap_or(ApiType::OpenaiCompletions);

                let model = Model {
                    id: mo.id.clone(),
                    name: mo.name.clone().unwrap_or_else(|| mo.id.clone()),
                    provider: mo.provider.clone(),
                    api,
                    context_window: mo.context_window.unwrap_or(128_000),
                    max_tokens: mo.max_tokens.unwrap_or(16_384),
                    reasoning: mo.reasoning.unwrap_or(false),
                    base_url: mo.base_url.clone(),
                    cost: CostConfig::default(),
                };

                // Replace existing model with same id+provider, or add new
                if let Some(pos) = self
                    .models
                    .iter()
                    .position(|m| m.id == model.id && m.provider == model.provider)
                {
                    self.models[pos] = model;
                } else {
                    self.models.push(model);
                }
            }
        }
    }

    /// Load additional models from a JSON file.
    /// The file should contain an array of model objects.
    pub fn load_from_file(&mut self, path: &Path) {
        if let Ok(content) = std::fs::read_to_string(path)
            && let Ok(models) = serde_json::from_str::<Vec<Model>>(&content)
        {
            for model in models {
                self.add(model);
            }
        }
    }

    /// Find a model using fuzzy matching.
    pub fn find_fuzzy(&self, pattern: &str, provider: Option<&str>) -> Option<&Model> {
        use crate::resolver::fuzzy_score;

        let pattern_lower = pattern.to_lowercase();
        let mut best: Option<(&Model, u32)> = None;

        for model in &self.models {
            if let Some(prov) = provider
                && model.provider != prov
            {
                continue;
            }

            let score = fuzzy_score(&pattern_lower, &model.id.to_lowercase())
                .max(fuzzy_score(&pattern_lower, &model.name.to_lowercase()));

            if score > 0 {
                match &best {
                    Some((_, bs)) if score > *bs => best = Some((model, score)),
                    None => best = Some((model, score)),
                    _ => {}
                }
            }
        }

        best.map(|(m, _)| m)
    }
}
