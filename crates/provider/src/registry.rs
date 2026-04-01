use serde::{Deserialize, Serialize};

/// Model definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub api: ApiType,
    pub context_window: u64,
    pub max_tokens: u64,
    pub reasoning: bool,
    pub base_url: Option<String>,
    pub cost: CostConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiType {
    OpenaiCompletions,
    OpenaiResponses,
    AnthropicMessages,
    GoogleGenerative,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CostConfig {
    pub input: f64,   // per million tokens
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Model registry holding all available models.
pub struct ModelRegistry {
    models: Vec<Model>,
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
}

fn builtin_models() -> Vec<Model> {
    vec![
        // Anthropic
        Model {
            id: "claude-sonnet-4-20250514".into(),
            name: "Claude Sonnet 4".into(),
            provider: "anthropic".into(),
            api: ApiType::AnthropicMessages,
            context_window: 200_000,
            max_tokens: 64_000,
            reasoning: true,
            base_url: Some("https://api.anthropic.com".into()),
            cost: CostConfig { input: 3.0, output: 15.0, cache_read: 0.3, cache_write: 3.75 },
        },
        Model {
            id: "claude-opus-4-20250514".into(),
            name: "Claude Opus 4".into(),
            provider: "anthropic".into(),
            api: ApiType::AnthropicMessages,
            context_window: 200_000,
            max_tokens: 32_000,
            reasoning: true,
            base_url: Some("https://api.anthropic.com".into()),
            cost: CostConfig { input: 15.0, output: 75.0, cache_read: 1.5, cache_write: 18.75 },
        },
        Model {
            id: "claude-haiku-4-5-20251001".into(),
            name: "Claude Haiku 4.5".into(),
            provider: "anthropic".into(),
            api: ApiType::AnthropicMessages,
            context_window: 200_000,
            max_tokens: 64_000,
            reasoning: true,
            base_url: Some("https://api.anthropic.com".into()),
            cost: CostConfig { input: 0.8, output: 4.0, cache_read: 0.08, cache_write: 1.0 },
        },
        // OpenAI
        Model {
            id: "gpt-4o".into(),
            name: "GPT-4o".into(),
            provider: "openai".into(),
            api: ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16_384,
            reasoning: false,
            base_url: Some("https://api.openai.com/v1".into()),
            cost: CostConfig { input: 2.5, output: 10.0, ..Default::default() },
        },
        Model {
            id: "gpt-4o-mini".into(),
            name: "GPT-4o Mini".into(),
            provider: "openai".into(),
            api: ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 16_384,
            reasoning: false,
            base_url: Some("https://api.openai.com/v1".into()),
            cost: CostConfig { input: 0.15, output: 0.6, ..Default::default() },
        },
        Model {
            id: "o3-mini".into(),
            name: "o3-mini".into(),
            provider: "openai".into(),
            api: ApiType::OpenaiCompletions,
            context_window: 200_000,
            max_tokens: 100_000,
            reasoning: true,
            base_url: Some("https://api.openai.com/v1".into()),
            cost: CostConfig { input: 1.1, output: 4.4, ..Default::default() },
        },
        // Google
        Model {
            id: "gemini-2.5-flash".into(),
            name: "Gemini 2.5 Flash".into(),
            provider: "google".into(),
            api: ApiType::GoogleGenerative,
            context_window: 1_000_000,
            max_tokens: 65_536,
            reasoning: true,
            base_url: Some("https://generativelanguage.googleapis.com".into()),
            cost: CostConfig { input: 0.15, output: 0.6, ..Default::default() },
        },
        Model {
            id: "gemini-2.5-pro".into(),
            name: "Gemini 2.5 Pro".into(),
            provider: "google".into(),
            api: ApiType::GoogleGenerative,
            context_window: 1_000_000,
            max_tokens: 65_536,
            reasoning: true,
            base_url: Some("https://generativelanguage.googleapis.com".into()),
            cost: CostConfig { input: 1.25, output: 10.0, ..Default::default() },
        },
        // Groq (OpenAI-compatible)
        Model {
            id: "llama-3.3-70b-versatile".into(),
            name: "Llama 3.3 70B".into(),
            provider: "groq".into(),
            api: ApiType::OpenaiCompletions,
            context_window: 128_000,
            max_tokens: 32_768,
            reasoning: false,
            base_url: Some("https://api.groq.com/openai/v1".into()),
            cost: CostConfig { input: 0.59, output: 0.79, ..Default::default() },
        },
        // OpenRouter (OpenAI-compatible)
        Model {
            id: "anthropic/claude-sonnet-4".into(),
            name: "Claude Sonnet 4 (OpenRouter)".into(),
            provider: "openrouter".into(),
            api: ApiType::OpenaiCompletions,
            context_window: 200_000,
            max_tokens: 64_000,
            reasoning: true,
            base_url: Some("https://openrouter.ai/api/v1".into()),
            cost: CostConfig { input: 3.0, output: 15.0, ..Default::default() },
        },
    ]
}
