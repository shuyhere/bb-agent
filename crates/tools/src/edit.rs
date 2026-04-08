use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use bb_core::types::ContentBlock;
use serde_json::{Value, json};
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;

use crate::{
    Tool, ToolContext, ToolResult, diff,
    path::{ensure_write_allowed, resolve_path},
};

#[cfg(test)]
mod tests;

pub struct EditTool;

struct PlannedEdit<'a> {
    index: usize,
    start: usize,
    end: usize,
    new_text: &'a str,
}

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

        let path = resolve_path(&ctx.cwd, path_str);
        ensure_write_allowed(ctx, &path, "edit")?;

        let edits = params
            .get("edits")
            .and_then(|v| v.as_array())
            .ok_or_else(|| BbError::Tool("Missing 'edits' array".into()))?;

        if edits.is_empty() {
            return Err(BbError::Tool("Empty edits array".into()));
        }

        let old_content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| BbError::Tool(format!("Failed to read {}: {e}", path.display())))?;

        let mut errors = Vec::new();
        let mut planned = Vec::new();

        for (i, edit) in edits.iter().enumerate() {
            let old_text = edit.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
            let new_text = edit.get("newText").and_then(|v| v.as_str()).unwrap_or("");

            if old_text.is_empty() {
                errors.push(format!("Edit {i}: oldText is empty"));
                continue;
            }

            let matches: Vec<_> = old_content.match_indices(old_text).collect();
            match matches.as_slice() {
                [] => {
                    errors.push(format!("Edit {i}: oldText not found in file"));
                }
                [(start, _matched)] => {
                    planned.push(PlannedEdit {
                        index: i,
                        start: *start,
                        end: *start + old_text.len(),
                        new_text,
                    });
                }
                _ => {
                    errors.push(format!(
                        "Edit {i}: oldText matches {} locations (must be unique)",
                        matches.len()
                    ));
                }
            }
        }

        let mut overlapping_edits = HashSet::new();
        for left in 0..planned.len() {
            for right in (left + 1)..planned.len() {
                let current = &planned[left];
                let other = &planned[right];
                let overlaps = current.start < other.end && other.start < current.end;
                if overlaps {
                    overlapping_edits.insert(current.index);
                    overlapping_edits.insert(other.index);
                    errors.push(format!(
                        "Edits {} and {} overlap in the original file; merge nearby changes into one edit",
                        current.index, other.index
                    ));
                }
            }
        }

        let mut content = old_content.clone();
        let mut applied = 0;
        planned.sort_by(|left, right| right.start.cmp(&left.start));
        for edit in planned {
            if overlapping_edits.contains(&edit.index) {
                continue;
            }
            content.replace_range(edit.start..edit.end, edit.new_text);
            applied += 1;
        }

        if applied > 0 {
            tokio::fs::write(&path, &content)
                .await
                .map_err(|e| BbError::Tool(format!("Failed to write {}: {e}", path.display())))?;
        }

        // Generate the diff once and reuse it
        let diff_str = if applied > 0 {
            let diff_lines = diff::generate_diff(&old_content, &content, 4);
            let rendered = diff::render_diff(&diff_lines);
            if rendered.is_empty() {
                None
            } else {
                Some(rendered.join("\n"))
            }
        } else {
            None
        };

        let mut msg = format!("Applied {applied}/{} edit(s) to {path_str}", edits.len());
        if let Some(ref diff) = diff_str {
            msg.push('\n');
            msg.push_str(diff);
        }
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
                "diff": diff_str,
            })),
            is_error: !errors.is_empty(),
            artifact_path: None,
        })
    }
}
