use std::path::Path;

use crate::config;

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

    if contents.is_empty() {
        None
    } else {
        Some(contents.join("\n\n---\n\n"))
    }
}
