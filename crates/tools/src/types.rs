use async_trait::async_trait;
use bb_core::error::BbResult;
use bb_provider::{Provider, registry::Model};
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;

/// Result from a tool execution.
#[derive(Clone, Debug)]
pub struct ToolResult {
    pub content: Vec<bb_core::types::ContentBlock>,
    pub details: Option<Value>,
    pub is_error: bool,
    pub artifact_path: Option<PathBuf>,
}

pub type OnOutputFn = Box<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct WebSearchRuntime {
    pub provider: Arc<dyn Provider>,
    pub model: Model,
    pub api_key: String,
    pub base_url: String,
    pub headers: HashMap<String, String>,
    pub enabled: bool,
}

/// Context available to tools during execution.
pub struct ToolContext {
    pub cwd: PathBuf,
    pub artifacts_dir: PathBuf,
    pub on_output: Option<OnOutputFn>,
    pub web_search: Option<WebSearchRuntime>,
}

/// Trait for built-in and custom tools.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;

    async fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> BbResult<ToolResult>;
}
