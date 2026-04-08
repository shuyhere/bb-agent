use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bb_core::error::{BbError, BbResult};

use crate::ToolContext;

pub(crate) fn resolve_path(cwd: &Path, path_str: &str) -> PathBuf {
    let path_str = path_str.strip_prefix('@').unwrap_or(path_str);
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

pub(crate) fn ensure_write_allowed(
    ctx: &ToolContext,
    path: &Path,
    operation: &str,
) -> BbResult<()> {
    if !ctx.execution_policy.restricts_workspace_writes() {
        return Ok(());
    }

    let workspace = std::fs::canonicalize(&ctx.cwd).unwrap_or_else(|_| ctx.cwd.clone());
    let target = canonicalize_target_for_write(path)?;

    if target.starts_with(&workspace) {
        Ok(())
    } else {
        Err(BbError::Tool(format!(
            "{operation} is restricted to the workspace in safety mode: {} is outside {}",
            target.display(),
            workspace.display()
        )))
    }
}

fn canonicalize_target_for_write(path: &Path) -> BbResult<PathBuf> {
    if path.exists() {
        return std::fs::canonicalize(path).map_err(BbError::from);
    }

    let mut missing_suffix = Vec::<OsString>::new();
    let mut current = path;

    while !current.exists() {
        let Some(name) = current.file_name() else {
            return Err(BbError::Tool(format!(
                "Cannot resolve write target outside filesystem root: {}",
                path.display()
            )));
        };
        missing_suffix.push(name.to_os_string());
        let Some(parent) = current.parent() else {
            return Err(BbError::Tool(format!(
                "Cannot resolve write target parent for {}",
                path.display()
            )));
        };
        current = parent;
    }

    let mut resolved = std::fs::canonicalize(current).map_err(BbError::from)?;
    for component in missing_suffix.iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}
