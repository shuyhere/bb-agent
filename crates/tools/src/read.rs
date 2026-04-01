use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use bb_core::types::ContentBlock;
use serde_json::{json, Value};
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolContext, ToolResult};

const DEFAULT_LIMIT: usize = 2000;
const MAX_BYTES: usize = 50 * 1024;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

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

        // Strip leading @ (some models add it)
        let path_str = path_str.strip_prefix('@').unwrap_or(path_str);
        let path = resolve_path(&ctx.cwd, path_str);

        if !path.exists() {
            return Err(BbError::Tool(format!("File not found: {}", path.display())));
        }

        // Check if image
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

async fn read_text(path: &Path, offset: usize, limit: usize) -> BbResult<ToolResult> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| BbError::Tool(format!("Failed to read {}: {e}", path.display())))?;

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let start = if offset > 0 { offset - 1 } else { 0 };
    let end = (start + limit).min(total_lines);

    if start >= total_lines {
        return Ok(ToolResult {
            content: vec![ContentBlock::Text {
                text: format!("File has {total_lines} lines. Offset {offset} is past end of file."),
            }],
            details: None,
            is_error: true,
            artifact_path: None,
        });
    }

    let selected: Vec<&str> = lines[start..end].to_vec();
    let mut output = selected.join("\n");

    // Truncate by bytes if needed
    if output.len() > MAX_BYTES {
        output.truncate(MAX_BYTES);
        if let Some(pos) = output.rfind('\n') {
            output.truncate(pos);
        }
    }

    let remaining = total_lines - end;
    if remaining > 0 {
        output.push_str(&format!(
            "\n\n[{remaining} more lines in file. Use offset={} to continue.]",
            end + 1
        ));
    }

    Ok(ToolResult {
        content: vec![ContentBlock::Text { text: output }],
        details: Some(json!({
            "path": path.display().to_string(),
            "totalLines": total_lines,
            "startLine": offset,
            "endLine": end,
        })),
        is_error: false,
        artifact_path: None,
    })
}

async fn read_image(path: &Path) -> BbResult<ToolResult> {
    let data = tokio::fs::read(path)
        .await
        .map_err(|e| BbError::Tool(format!("Failed to read image: {e}")))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();

    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);

    Ok(ToolResult {
        content: vec![ContentBlock::Image {
            data: encoded,
            mime_type: mime.to_string(),
        }],
        details: None,
        is_error: false,
        artifact_path: None,
    })
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn resolve_path(cwd: &Path, path_str: &str) -> std::path::PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}
