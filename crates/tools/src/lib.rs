pub mod artifacts;
pub mod read;
pub mod bash;
pub mod diff;
pub mod edit;
pub mod find;
pub mod grep;
pub mod ls;
pub mod write;
pub mod scheduler;

use async_trait::async_trait;
use bb_core::error::BbResult;
use serde_json::Value;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

/// Result from a tool execution.
#[derive(Clone, Debug)]
pub struct ToolResult {
    pub content: Vec<bb_core::types::ContentBlock>,
    pub details: Option<Value>,
    pub is_error: bool,
    pub artifact_path: Option<PathBuf>,
}

/// Context available to tools during execution.
pub struct ToolContext {
    pub cwd: PathBuf,
    pub artifacts_dir: PathBuf,
    pub on_output: Option<Box<dyn Fn(&str) + Send + Sync>>,
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

/// Get all built-in tools.
pub fn builtin_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(read::ReadTool),
        Box::new(bash::BashTool),
        Box::new(edit::EditTool),
        Box::new(write::WriteTool),
        Box::new(find::FindTool),
        Box::new(grep::GrepTool),
        Box::new(ls::LsTool),
    ]
}
