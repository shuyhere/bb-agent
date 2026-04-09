use serde::{Deserialize, Serialize};

fn default_model_inputs() -> Vec<ModelInput> {
    vec![ModelInput::Text]
}

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
    #[serde(default = "default_model_inputs")]
    pub input: Vec<ModelInput>,
    pub base_url: Option<String>,
    pub cost: CostConfig,
}

impl Model {
    pub fn supports_images(&self) -> bool {
        self.input.contains(&ModelInput::Image)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelInput {
    Text,
    Image,
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
    pub input: f64, // per million tokens
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}
