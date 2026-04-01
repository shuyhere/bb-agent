use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use bb_core::types::ContentBlock;
use serde_json::{json, Value};
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolContext, ToolResult};

pub struct EditTool;

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "edit"
    }

    fn description(&self) -> &str {
        "Edit a single file using exact text replacement. Every oldText must match a unique, \
         non-overlapping region of the original file."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to edit" },
                "edits": {
                    "type": "array",
                    "description": "One or more targeted replacements",
                    "items": {
                        "type": "object",
                        "properties": {
                            "oldText": { "type": "string", "description": "Exact text to find" },
                            "newText": { "type": "string", "description": "Replacement text" }
                        },
                        "required": ["oldText", "newText"]
                    }
                }
            },
            "required": ["path", "edits"]
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

        let path_str = path_str.strip_prefix('@').unwrap_or(path_str);
        let path = if Path::new(path_str).is_absolute() {
            Path::new(path_str).to_path_buf()
        } else {
            ctx.cwd.join(path_str)
        };

        let edits = params
            .get("edits")
            .and_then(|v| v.as_array())
            .ok_or_else(|| BbError::Tool("Missing 'edits' array".into()))?;

        if edits.is_empty() {
            return Err(BbError::Tool("Empty edits array".into()));
        }

        let mut content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| BbError::Tool(format!("Failed to read {}: {e}", path.display())))?;

        let mut applied = 0;
        let mut errors = Vec::new();

        for (i, edit) in edits.iter().enumerate() {
            let old_text = edit
                .get("oldText")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let new_text = edit
                .get("newText")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if old_text.is_empty() {
                errors.push(format!("Edit {i}: oldText is empty"));
                continue;
            }

            let count = content.matches(old_text).count();
            if count == 0 {
                errors.push(format!("Edit {i}: oldText not found in file"));
                continue;
            }
            if count > 1 {
                errors.push(format!("Edit {i}: oldText matches {count} locations (must be unique)"));
                continue;
            }

            content = content.replacen(old_text, new_text, 1);
            applied += 1;
        }

        if applied > 0 {
            tokio::fs::write(&path, &content)
                .await
                .map_err(|e| BbError::Tool(format!("Failed to write {}: {e}", path.display())))?;
        }

        let mut msg = format!("Applied {applied}/{} edit(s) to {path_str}", edits.len());
        if !errors.is_empty() {
            msg.push_str(&format!("\nErrors:\n{}", errors.join("\n")));
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text { text: msg }],
            details: Some(json!({
                "path": path_str,
                "applied": applied,
                "total": edits.len(),
                "errors": errors,
            })),
            is_error: !errors.is_empty(),
            artifact_path: None,
        })
    }
}
