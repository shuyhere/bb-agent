use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    Tool, ToolContext, ToolResult,
    path::{ensure_write_allowed, resolve_path},
    support::text_result,
};

#[cfg(test)]
mod tests;

pub struct WriteTool;

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "write"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. \
         Automatically creates parent directories."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to write" },
                "content": { "type": "string", "description": "Content to write to the file" }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
        _cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BbError::Tool("Missing 'path' parameter".into()))?;
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BbError::Tool("Missing 'content' parameter".into()))?;

        let path = resolve_path(&ctx.cwd, path_str);
        ensure_write_allowed(ctx, &path, "write")?;

        // Create parent directories
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| BbError::Tool(format!("Failed to create directories: {e}")))?;
        }

        let bytes = content.len();
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| BbError::Tool(format!("Failed to write {}: {e}", path.display())))?;

        Ok(text_result(
            format!("Successfully wrote {bytes} bytes to {path_str}"),
            Some(json!({
                "path": path_str,
                "bytes": bytes,
            })),
        ))
    }
}
