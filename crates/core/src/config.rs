use std::path::{Path, PathBuf};

const PROJECT_ROOT_MARKERS: &[&str] = &[
    ".git",
    "Cargo.toml",
    "package.json",
    "go.mod",
    "pyproject.toml",
    ".hg",
    "AGENTS.md",
    "CLAUDE.md",
];

/// Resolve the global BB-Agent directory.
pub fn global_dir() -> PathBuf {
    if let Some(home) = home_dir() {
        home.join(".bb-agent")
    } else {
        PathBuf::from(".bb-agent")
    }
}

/// Find the effective project root for `start` by walking ancestors.
///
/// Markers include common repository files (`.git`, `Cargo.toml`, `package.json`, etc.)
/// plus an explicit project-local `.bb-agent/settings.json`.
///
/// The global home-level `~/.bb-agent/settings.json` is intentionally *not* treated as a
/// project marker, so running inside a subdirectory of `$HOME` does not accidentally load
/// global settings as project settings.
pub fn project_root(start: &Path) -> Option<PathBuf> {
    let start = normalize_path(start);
    let home = home_dir().map(|path| normalize_path(&path));

    for dir in start.ancestors() {
        if has_project_marker(dir, home.as_deref()) {
            return Some(dir.to_path_buf());
        }
    }
    None
}

/// Resolve the project-local BB-Agent directory using the discovered project root when possible.
/// Falls back to the provided `cwd` if no project root markers are found.
pub fn project_dir(cwd: &Path) -> PathBuf {
    project_root(cwd)
        .unwrap_or_else(|| normalize_path(cwd))
        .join(".bb-agent")
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn has_project_marker(dir: &Path, home: Option<&Path>) -> bool {
    if PROJECT_ROOT_MARKERS
        .iter()
        .any(|marker| dir.join(marker).exists())
    {
        return true;
    }

    let explicit_project_settings = dir.join(".bb-agent").join("settings.json");
    if explicit_project_settings.exists() {
        if let Some(home) = home
            && dir == home
        {
            return false;
        }
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn make_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bb-config-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn project_root_finds_repo_marker_in_ancestor() {
        let root = make_temp_dir();
        fs::write(root.join("Cargo.toml"), "[package]\nname='demo'\n").unwrap();
        let nested = root.join("src").join("deep");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(project_root(&nested).as_deref(), Some(root.as_path()));
        assert_eq!(project_dir(&nested), root.join(".bb-agent"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_root_finds_explicit_project_settings_in_ancestor() {
        let root = make_temp_dir();
        fs::create_dir_all(root.join(".bb-agent")).unwrap();
        fs::write(root.join(".bb-agent").join("settings.json"), "{}\n").unwrap();
        let nested = root.join("a").join("b");
        fs::create_dir_all(&nested).unwrap();

        assert_eq!(project_root(&nested).as_deref(), Some(root.as_path()));

        let _ = fs::remove_dir_all(root);
    }
}
