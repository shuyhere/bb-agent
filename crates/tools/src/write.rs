use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use bb_core::types::ContentBlock;
use serde_json::{json, Value};
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolContext, ToolResult};

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

        let path_str = path_str.strip_prefix('@').unwrap_or(path_str);
        let path = if Path::new(path_str).is_absolute() {
            Path::new(path_str).to_path_buf()
        } else {
            ctx.cwd.join(path_str)
        };

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

        Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("Successfully wrote {bytes} bytes to {path_str}"),
            }],
            details: Some(json!({
                "path": path_str,
                "bytes": bytes,
            })),
            is_error: false,
            artifact_path: None,
        })
    }
}
