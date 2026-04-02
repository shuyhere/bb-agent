use crate::types::CompactionSettings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Global + project settings, merged.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub compaction: CompactionSettings,
}

impl Settings {
    /// Parse settings from a JSON string, returning defaults on invalid input.
    pub fn parse(content: &str) -> Self {
        serde_json::from_str(content).unwrap_or_default()
    }

    // IO boundary — should migrate to cli
    /// Load settings from a JSON file, or return defaults.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => Self::parse(&content),
            Err(_) => Self::default(),
        }
    }

    /// Merge project settings on top of global settings.
    /// Project values override global where present.
    pub fn merge(global: &Self, project: &Self) -> Self {
        // For now, project fully overrides. Can be made more granular later.
        Self {
            compaction: CompactionSettings {
                enabled: project.compaction.enabled,
                reserve_tokens: if project.compaction.reserve_tokens != 16384 {
                    project.compaction.reserve_tokens
                } else {
                    global.compaction.reserve_tokens
                },
                keep_recent_tokens: if project.compaction.keep_recent_tokens != 20000 {
                    project.compaction.keep_recent_tokens
                } else {
                    global.compaction.keep_recent_tokens
                },
            },
        }
    }
}

/// Resolve the global bb-agent directory.
pub fn global_dir() -> PathBuf {
    dirs_or_default()
}

/// Resolve project-local bb-agent directory.
pub fn project_dir(cwd: &Path) -> PathBuf {
    cwd.join(".bb-agent")
}

fn dirs_or_default() -> PathBuf {
    if let Some(home) = home_dir() {
        home.join(".bb-agent")
    } else {
        PathBuf::from(".bb-agent")
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
