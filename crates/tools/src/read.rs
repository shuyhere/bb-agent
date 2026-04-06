use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolContext, ToolResult, path::resolve_path};

mod image;
#[cfg(test)]
mod tests;
mod text;

use image::{is_image, read_image};
use text::read_text;
#[cfg(test)]
use text::safe_char_boundary_at_or_before;

const DEFAULT_LIMIT: usize = 2000;

pub struct ReadTool;

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file. Supports text files and images (jpg, png, gif, webp). \
         Images are sent as attachments. For text files, output is truncated to 2000 lines \
         or 50KB (whichever is hit first). Use offset/limit for large files."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to read (relative or absolute)" },
                "offset": { "type": "number", "description": "Line number to start reading from (1-indexed)" },
                "limit": { "type": "number", "description": "Maximum number of lines to read" }
            },
            "required": ["path"]
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

        let path = resolve_path(&ctx.cwd, path_str);

        if !path.exists() {
            return Err(BbError::Tool(format!("File not found: {}", path.display())));
        }

        if is_image(&path) {
            return read_image(&path).await;
        }

        let offset = params
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(1);
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LIMIT);

        read_text(&path, offset, limit).await
    }
}
