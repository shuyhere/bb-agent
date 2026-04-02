use std::path::Path;

use crate::config;

/// Pure logic: merge multiple AGENTS.md / CLAUDE.md content strings into one.
/// The input should be ordered from most-global to most-local.
/// Returns `None` if the input is empty.
pub fn merge_agents_md_contents(contents: &[String]) -> Option<String> {
    if contents.is_empty() {
        None
    } else {
        Some(contents.join("\n\n---\n\n"))
    }
}

// IO boundary — should migrate to cli
pub fn load_agents_md(cwd: &Path) -> Option<String> {
    let mut contents = Vec::new();

    let global = config::global_dir().join("AGENTS.md");
    if global.exists() {
        if let Ok(content) = std::fs::read_to_string(&global) {
            contents.push(content);
        }
    }

    let mut dir = cwd.to_path_buf();
    let mut scanned = Vec::new();
    loop {
        let agents = dir.join("AGENTS.md");
        if agents.exists() {
            scanned.push(agents);
        } else {
            let claude = dir.join("CLAUDE.md");
            if claude.exists() {
                scanned.push(claude);
            }
        }

        if dir.join(".git").exists() {
            break;
        }
        if !dir.pop() {
            break;
        }
    }

    scanned.reverse();
    for path in scanned {
        if let Ok(content) = std::fs::read_to_string(&path) {
            contents.push(content);
        }
    }

    merge_agents_md_contents(&contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_empty() {
        assert_eq!(merge_agents_md_contents(&[]), None);
    }

    #[test]
    fn test_merge_single() {
        let contents = vec!["# Global rules".to_string()];
        assert_eq!(
            merge_agents_md_contents(&contents),
            Some("# Global rules".to_string())
        );
    }

    #[test]
    fn test_merge_multiple() {
        let contents = vec![
            "# Global".to_string(),
            "# Project".to_string(),
            "# Local".to_string(),
        ];
        assert_eq!(
            merge_agents_md_contents(&contents),
            Some("# Global\n\n---\n\n# Project\n\n---\n\n# Local".to_string())
        );
    }
}
