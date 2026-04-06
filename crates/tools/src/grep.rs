use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use serde_json::{Value, json};
use std::path::Path;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use crate::{
    Tool, ToolContext, ToolResult, path::resolve_path, support::text_result,
    text::format_limited_results,
};

const DEFAULT_LIMIT: usize = 100;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search file contents using ripgrep (rg). Falls back to grep -rn if rg is unavailable. \
         Returns matching lines with file:line: prefix."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Search pattern (regex or literal string)"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search (default: current directory)"
                },
                "glob": {
                    "type": "string",
                    "description": "Filter files by glob pattern, e.g. '*.rs' or '**/*.spec.ts'"
                },
                "ignoreCase": {
                    "type": "boolean",
                    "description": "Case-insensitive search (default: false)"
                },
                "literal": {
                    "type": "boolean",
                    "description": "Treat pattern as literal string instead of regex (default: false)"
                },
                "context": {
                    "type": "number",
                    "description": "Number of lines to show before and after each match (default: 0)"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of matches to return (default: 100)"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
        _cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let pattern = params
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| BbError::Tool("Missing 'pattern' parameter".into()))?;

        let search_path = params
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve_path(&ctx.cwd, p))
            .unwrap_or_else(|| ctx.cwd.clone());

        let glob_filter = params.get("glob").and_then(|v| v.as_str());
        let ignore_case = params
            .get("ignoreCase")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let literal = params
            .get("literal")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let context_lines = params
            .get("context")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(0);
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LIMIT);

        if !search_path.exists() {
            return Err(BbError::Tool(format!(
                "Path not found: {}",
                search_path.display()
            )));
        }

        // Try rg first
        match grep_with_rg(
            pattern,
            &search_path,
            glob_filter,
            ignore_case,
            literal,
            context_lines,
            limit,
        )
        .await
        {
            Ok(results) => format_results(results, limit),
            Err(_) => {
                // Fall back to grep -rn
                match grep_with_grep_cmd(
                    pattern,
                    &search_path,
                    ignore_case,
                    literal,
                    context_lines,
                    limit,
                )
                .await
                {
                    Ok(results) => format_results(results, limit),
                    Err(e) => Err(BbError::Tool(format!("Grep failed: {e}"))),
                }
            }
        }
    }
}

async fn grep_with_rg(
    pattern: &str,
    path: &Path,
    glob_filter: Option<&str>,
    ignore_case: bool,
    literal: bool,
    context_lines: usize,
    limit: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut cmd = Command::new("rg");
    cmd.arg("--line-number")
        .arg("--no-heading")
        .arg("--max-count")
        .arg(limit.to_string());

    if ignore_case {
        cmd.arg("--ignore-case");
    }
    if literal {
        cmd.arg("--fixed-strings");
    }
    if context_lines > 0 {
        cmd.arg("--context").arg(context_lines.to_string());
    }
    if let Some(glob) = glob_filter {
        cmd.arg("--glob").arg(glob);
    }

    cmd.arg(pattern).arg(path);

    let output = cmd.output().await?;

    // rg returns exit code 1 for "no matches" — that's not an error
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("rg failed: {stderr}").into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let results: Vec<String> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .take(limit)
        .map(|l| l.to_string())
        .collect();
    Ok(results)
}

async fn grep_with_grep_cmd(
    pattern: &str,
    path: &Path,
    ignore_case: bool,
    literal: bool,
    context_lines: usize,
    limit: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut cmd = Command::new("grep");
    cmd.arg("-rn");

    if ignore_case {
        cmd.arg("-i");
    }
    if literal {
        cmd.arg("-F");
    }
    if context_lines > 0 {
        cmd.arg("-C").arg(context_lines.to_string());
    }

    cmd.arg(pattern).arg(path);

    let output = cmd.output().await?;

    // grep returns exit code 1 for "no matches"
    if !output.status.success() && output.status.code() != Some(1) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("grep failed: {stderr}").into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let results: Vec<String> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .take(limit)
        .map(|l| l.to_string())
        .collect();
    Ok(results)
}

fn format_results(results: Vec<String>, limit: usize) -> BbResult<ToolResult> {
    let total = results.len();
    let (text, truncated) = format_limited_results(&results, "No matches found.", limit);

    Ok(text_result(
        text,
        Some(json!({
            "matchCount": total,
            "truncated": truncated,
        })),
    ))
}
