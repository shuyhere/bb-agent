use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::settings_defaults::{
    default_enable_skill_commands, default_keep, default_reserve, default_retry_delay,
    default_retry_max, default_retry_max_delay, default_true, default_update_check_ttl_hours,
};
pub use crate::settings_packages::{PackageEntry, PackageFilter};

mod io;
mod merge;
#[cfg(test)]
mod tests;

/// Layered settings: global (`~/.bb-agent/settings.json`) merged with
/// project (`.bb-agent/settings.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub compaction: CompactionConfig,
    #[serde(default)]
    pub retry: RetryConfig,
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub default_thinking: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub prompts: Vec<String>,
    #[serde(default)]
    pub packages: Vec<PackageEntry>,
    #[serde(
        default = "default_enable_skill_commands",
        alias = "enableSkillCommands"
    )]
    pub enable_skill_commands: bool,
    #[serde(default)]
    pub models: Option<Vec<ModelOverride>>,
    #[serde(default)]
    pub providers: Option<Vec<ProviderOverride>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_theme: Option<String>,
    #[serde(default, alias = "updateCheck")]
    pub update_check: UpdateCheckSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompactionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_reserve")]
    pub reserve_tokens: u64,
    #[serde(default = "default_keep")]
    pub keep_recent_tokens: u64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            reserve_tokens: default_reserve(),
            keep_recent_tokens: default_keep(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_retry_max")]
    pub max_retries: u32,
    #[serde(default = "default_retry_delay")]
    pub base_delay_ms: u64,
    #[serde(default = "default_retry_max_delay")]
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            max_retries: default_retry_max(),
            base_delay_ms: default_retry_delay(),
            max_delay_ms: default_retry_max_delay(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateCheckSettings {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_update_check_ttl_hours", alias = "ttlHours")]
    pub ttl_hours: u64,
}

impl Default for UpdateCheckSettings {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            ttl_hours: default_update_check_ttl_hours(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelOverride {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub provider: String,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub reasoning: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderOverride {
    pub name: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            compaction: CompactionConfig::default(),
            retry: RetryConfig::default(),
            default_provider: None,
            default_model: None,
            default_thinking: None,
            tools: None,
            extensions: Vec::new(),
            skills: Vec::new(),
            prompts: Vec::new(),
            packages: Vec::new(),
            enable_skill_commands: default_enable_skill_commands(),
            models: None,
            providers: None,
            color_theme: None,
            update_check: UpdateCheckSettings::default(),
        }
    }
}

impl Settings {
    /// Parse settings from a JSON string.
    pub fn parse_result(content: &str) -> std::io::Result<Self> {
        serde_json::from_str(content).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse settings JSON: {error}"),
            )
        })
    }

    /// Parse settings from a JSON string, returning defaults on invalid input.
    /// Prefer `parse_result()` when callers need to surface malformed config.
    pub fn parse(content: &str) -> Self {
        Self::parse_result(content).unwrap_or_default()
    }

    /// Merge project settings on top of global settings.
    /// Project values override global when present (non-None / non-default).
    pub fn merge(global: &Self, project: &Self) -> Self {
        merge::merge_settings(global, project)
    }

    /// Convert compaction config to the core CompactionSettings type.
    pub fn compaction_settings(&self) -> crate::types::CompactionSettings {
        crate::types::CompactionSettings {
            enabled: self.compaction.enabled,
            reserve_tokens: self.compaction.reserve_tokens,
            keep_recent_tokens: self.compaction.keep_recent_tokens,
        }
    }
}
