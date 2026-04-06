use bb_core::error::{BbError, BbResult};
use serde_json::json;
use std::path::Path;

use crate::{
    ToolResult,
    support::{text_result, text_result_with},
};

const MAX_BYTES: usize = 50 * 1024;

pub(super) fn safe_char_boundary_at_or_before(s: &str, max_bytes: usize) -> usize {
    let capped = max_bytes.min(s.len());
    if s.is_char_boundary(capped) {
        return capped;
    }
    let mut last = 0usize;
    for (idx, _) in s.char_indices() {
        if idx > capped {
            break;
        }
        last = idx;
    }
    last
}

pub(super) async fn read_text(path: &Path, offset: usize, limit: usize) -> BbResult<ToolResult> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| BbError::Tool(format!("Failed to read {}: {e}", path.display())))?;

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let start = if offset > 0 { offset - 1 } else { 0 };
    let end = (start + limit).min(total_lines);

    if start >= total_lines {
        return Ok(text_result_with(
            format!("File has {total_lines} lines. Offset {offset} is past end of file."),
            None,
            true,
            None,
        ));
    }

    let selected: Vec<&str> = lines[start..end].to_vec();
    let mut output = selected.join("\n");

    if output.len() > MAX_BYTES {
        let safe_cut = safe_char_boundary_at_or_before(&output, MAX_BYTES);
        output.truncate(safe_cut);
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

    Ok(text_result(
        output,
        Some(json!({
            "path": path.display().to_string(),
            "totalLines": total_lines,
            "startLine": offset,
            "endLine": end,
        })),
    ))
}
