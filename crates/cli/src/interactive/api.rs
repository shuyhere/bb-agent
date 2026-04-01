use bb_provider::Provider;
use bb_tools::{Tool, ToolContext};
use std::error::Error;

pub type InteractiveResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Debug, Default)]
pub struct InteractiveModeOptions {
    pub verbose: bool,
    pub quiet_startup: bool,
    pub migrated_providers: Vec<String>,
    pub model_fallback_message: Option<String>,
    pub initial_message: Option<String>,
    pub initial_images: Vec<String>,
    pub initial_messages: Vec<String>,
    pub session_id: Option<String>,
    pub model_display: Option<String>,
    pub agents_md: Option<String>,
}

/// Non-Clone runtime state needed for actual LLM calls.
pub struct InteractiveSessionSetup {
    pub conn: rusqlite::Connection,
    pub session_id: String,
    pub provider: Box<dyn Provider>,
    pub model: bb_provider::registry::Model,
    pub api_key: String,
    pub base_url: String,
    pub tools: Vec<Box<dyn Tool>>,
    pub tool_defs: Vec<serde_json::Value>,
    pub tool_ctx: ToolContext,
    pub system_prompt: String,
    pub thinking_level: String,
}
