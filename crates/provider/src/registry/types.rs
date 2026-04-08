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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApiType {
    #[default]
    OpenaiCompletions,
    OpenaiResponses,
    AnthropicMessages,
    GoogleGenerative,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CostConfig {
    pub input: f64, // per million tokens
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}
