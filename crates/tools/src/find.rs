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

const DEFAULT_LIMIT: usize = 1000;

pub struct FindTool;

#[async_trait]
impl Tool for FindTool {
    fn name(&self) -> &str {
        "find"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Uses fd for fast searching with .gitignore support, \
         falls back to a basic find command."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files, e.g. '*.rs', '**/*.json', or 'src/**/*.spec.ts'"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in (default: current directory)"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of results (default: 1000)"
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

        let search_dir = params
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve_path(&ctx.cwd, p))
            .unwrap_or_else(|| ctx.cwd.clone());

        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LIMIT);

        if !search_dir.exists() {
            return Err(BbError::Tool(format!(
                "Directory not found: {}",
                search_dir.display()
            )));
        }

        // Try fd first
        match find_with_fd(pattern, &search_dir, limit).await {
            Ok(results) => format_results(results, limit),
            Err(_) => {
                // Fall back to basic find command
                match find_with_find_cmd(pattern, &search_dir, limit).await {
                    Ok(results) => format_results(results, limit),
                    Err(e) => Err(BbError::Tool(format!("Find failed: {e}"))),
                }
            }
        }
    }
}

async fn find_with_fd(
    pattern: &str,
    dir: &Path,
    limit: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let output = Command::new("fd")
        .arg("--glob")
        .arg(pattern)
        .arg("--max-results")
        .arg(limit.to_string())
        .arg("--type")
        .arg("f")
        .current_dir(dir)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("fd failed: {stderr}").into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let results: Vec<String> = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    Ok(results)
}

async fn find_with_find_cmd(
    pattern: &str,
    dir: &Path,
    limit: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let output = Command::new("find")
        .arg(dir)
        .arg("-type")
        .arg("f")
        .arg("-name")
        .arg(pattern)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("find failed: {stderr}").into());
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
    let (text, truncated) = format_limited_results(&results, "No files found.", limit);

    Ok(text_result(
        text,
        Some(json!({
            "matchCount": total,
            "truncated": truncated,
        })),
    ))
}
