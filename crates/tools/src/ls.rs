use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use bb_core::types::ContentBlock;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolContext, ToolResult};

const DEFAULT_LIMIT: usize = 200;
const MAX_DEPTH: usize = 3;

pub struct LsTool;

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List directory contents in a tree-like format. Recursively lists up to depth 3. \
         Respects .gitignore when possible."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory to list (default: current directory)"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of entries to return (default: 500)"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
        _cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let dir = params
            .get("path")
            .and_then(|v| v.as_str())
            .map(|p| resolve_path(&ctx.cwd, p))
            .unwrap_or_else(|| ctx.cwd.clone());

        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LIMIT);

        if !dir.exists() {
            return Err(BbError::Tool(format!(
                "Directory not found: {}",
                dir.display()
            )));
        }

        if !dir.is_dir() {
            return Err(BbError::Tool(format!(
                "Not a directory: {}",
                dir.display()
            )));
        }

        // Load .gitignore patterns for the directory
        let gitignore = load_gitignore(&dir);

        let mut entries = Vec::new();
        let mut count = 0;
        let truncated = list_dir_recursive(
            &dir,
            &dir,
            "",
            0,
            MAX_DEPTH,
            limit,
            &mut count,
            &mut entries,
            &gitignore,
        );

        let mut text = if entries.is_empty() {
            "Directory is empty.".to_string()
        } else {
            entries.join("\n")
        };

        if truncated {
            text.push_str(&format!("\n\n[Output truncated at {limit} entries]"));
        }

        Ok(ToolResult {
            content: vec![ContentBlock::Text { text }],
            details: Some(json!({
                "entryCount": count,
                "truncated": truncated,
            })),
            is_error: false,
            artifact_path: None,
        })
    }
}

/// Recursively list directory contents with tree-like indentation.
/// Returns true if the limit was reached.
fn list_dir_recursive(
    root: &Path,
    dir: &Path,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    limit: usize,
    count: &mut usize,
    entries: &mut Vec<String>,
    gitignore: &[String],
) -> bool {
    if depth > max_depth {
        return false;
    }

    let mut dir_entries: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).collect(),
        Err(_) => return false,
    };

    // Sort entries: directories first, then alphabetically
    dir_entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    let total = dir_entries.len();
    for (i, entry) in dir_entries.iter().enumerate() {
        if *count >= limit {
            return true;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs (starting with .)
        if name.starts_with('.') {
            continue;
        }

        // Skip gitignored entries
        if is_gitignored(&name, gitignore) {
            continue;
        }

        let is_last = i == total - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

        let display_name = if is_dir {
            format!("{name}/")
        } else {
            name.clone()
        };

        entries.push(format!("{prefix}{connector}{display_name}"));
        *count += 1;

        if is_dir && depth < max_depth {
            let child_prefix = if is_last {
                format!("{prefix}    ")
            } else {
                format!("{prefix}│   ")
            };
            let truncated = list_dir_recursive(
                root,
                &entry.path(),
                &child_prefix,
                depth + 1,
                max_depth,
                limit,
                count,
                entries,
                gitignore,
            );
            if truncated {
                return true;
            }
        }
    }

    false
}

/// Load simple .gitignore patterns from a directory.
fn load_gitignore(dir: &Path) -> Vec<String> {
    let gitignore_path = dir.join(".gitignore");
    match fs::read_to_string(gitignore_path) {
        Ok(content) => content
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| l.trim_end_matches('/').to_string())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Simple gitignore matching — checks if a name matches any pattern.
fn is_gitignored(name: &str, patterns: &[String]) -> bool {
    for pattern in patterns {
        if pattern == name {
            return true;
        }
        // Simple glob: pattern like *.ext
        if let Some(suffix) = pattern.strip_prefix('*') {
            if name.ends_with(suffix) {
                return true;
            }
        }
    }
    false
}

fn resolve_path(cwd: &Path, path_str: &str) -> std::path::PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}
