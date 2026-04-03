use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

// =============================================================================
// Helper defaults for serde
// =============================================================================

fn default_true() -> bool {
    true
}

fn default_reserve() -> u64 {
    16384
}

fn default_keep() -> u64 {
    20000
}

fn default_retry_max() -> u32 {
    3
}

fn default_retry_delay() -> u64 {
    2000
}

fn default_retry_max_delay() -> u64 {
    60000
}

fn default_enable_skill_commands() -> bool {
    true
}

// =============================================================================
// Package entry types
// =============================================================================

/// A package entry in settings — either a simple source string or a
/// filtered object with per-resource-type filters.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PackageEntry {
    Simple(String),
    Filtered(PackageFilter),
}

impl PackageEntry {
    /// Get the package source string.
    pub fn source(&self) -> &str {
        match self {
            PackageEntry::Simple(s) => s,
            PackageEntry::Filtered(f) => &f.source,
        }
    }

    /// Get the optional filter for extensions.
    pub fn extensions_filter(&self) -> Option<&[String]> {
        match self {
            PackageEntry::Simple(_) => None,
            PackageEntry::Filtered(f) => f.extensions.as_deref(),
        }
    }

    /// Get the optional filter for skills.
    pub fn skills_filter(&self) -> Option<&[String]> {
        match self {
            PackageEntry::Simple(_) => None,
            PackageEntry::Filtered(f) => f.skills.as_deref(),
        }
    }

    /// Get the optional filter for prompts.
    pub fn prompts_filter(&self) -> Option<&[String]> {
        match self {
            PackageEntry::Simple(_) => None,
            PackageEntry::Filtered(f) => f.prompts.as_deref(),
        }
    }
}

impl From<String> for PackageEntry {
    fn from(s: String) -> Self {
        PackageEntry::Simple(s)
    }
}

impl From<&str> for PackageEntry {
    fn from(s: &str) -> Self {
        PackageEntry::Simple(s.to_string())
    }
}

/// Filtered package entry with optional per-resource-type filters.
///
/// Filters layer on top of the manifest. They narrow down what is already
/// allowed:
/// - `None` (omitted key) = load all of that type
/// - `[]` (empty array) = load none of that type
/// - Patterns match relative paths from the package root
/// - `!pattern` excludes matches
/// - `+path` force-includes an exact path
/// - `-path` force-excludes an exact path
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PackageFilter {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Vec<String>>,
}

// =============================================================================
// Settings
// =============================================================================

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
        }
    }
}

impl Settings {
    /// Parse settings from a JSON string, returning defaults on invalid input.
    pub fn parse(content: &str) -> Self {
        serde_json::from_str(content).unwrap_or_default()
    }

    // IO boundary — should migrate to cli
    /// Load the global settings from `~/.bb-agent/settings.json`.
    pub fn load_global() -> Self {
        let dir = crate::config::global_dir();
        let path = dir.join("settings.json");
        Self::load_from_file(&path)
    }

    /// Save global settings to `~/.bb-agent/settings.json`.
    pub fn save_global(&self) -> std::io::Result<()> {
        let dir = crate::config::global_dir();
        self.save_to_file(&dir.join("settings.json"))
    }

    /// Save project settings to `<cwd>/.bb-agent/settings.json`.
    pub fn save_project(&self, cwd: &Path) -> std::io::Result<()> {
        self.save_to_file(&cwd.join(".bb-agent").join("settings.json"))
    }

    /// Save settings to a specific file path.
    pub fn save_to_file(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, content)
    }

    // IO boundary — should migrate to cli
    /// Load project-local settings by walking up from `cwd` looking for
    /// `.bb-agent/settings.json`.
    pub fn load_project(cwd: &Path) -> Self {
        let path = cwd.join(".bb-agent").join("settings.json");
        Self::load_from_file(&path)
    }

    // IO boundary — should migrate to cli
    /// Load settings from a specific file path, returning defaults if the
    /// file doesn't exist or can't be parsed.
    pub fn load_from_file(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => Self::parse(&content),
            Err(_) => Self::default(),
        }
    }

    /// Merge project settings on top of global settings.
    /// Project values override global when present (non-None / non-default).
    pub fn merge(global: &Self, project: &Self) -> Self {
        Self {
            compaction: merge_compaction(&global.compaction, &project.compaction),
            retry: merge_retry(&global.retry, &project.retry),
            default_provider: project
                .default_provider
                .clone()
                .or_else(|| global.default_provider.clone()),
            default_model: project
                .default_model
                .clone()
                .or_else(|| global.default_model.clone()),
            default_thinking: project
                .default_thinking
                .clone()
                .or_else(|| global.default_thinking.clone()),
            tools: project.tools.clone().or_else(|| global.tools.clone()),
            extensions: merge_string_lists(&global.extensions, &project.extensions),
            skills: merge_string_lists(&global.skills, &project.skills),
            prompts: merge_string_lists(&global.prompts, &project.prompts),
            packages: merge_package_lists(&global.packages, &project.packages),
            enable_skill_commands: merge_bool_with_default(
                global.enable_skill_commands,
                project.enable_skill_commands,
                default_enable_skill_commands(),
            ),
            models: merge_optional_vec(&global.models, &project.models),
            providers: merge_optional_vec_providers(&global.providers, &project.providers),
        }
    }

    // IO boundary — should migrate to cli
    /// Convenience: load global + project and merge.
    pub fn load_merged(cwd: &Path) -> Self {
        let global = Self::load_global();
        let project = Self::load_project(cwd);
        Self::merge(&global, &project)
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

// =============================================================================
// Merge helpers
// =============================================================================

fn merge_compaction(global: &CompactionConfig, project: &CompactionConfig) -> CompactionConfig {
    let defaults = CompactionConfig::default();
    CompactionConfig {
        enabled: if !project.enabled && defaults.enabled {
            false
        } else {
            project.enabled
        },
        reserve_tokens: if project.reserve_tokens != defaults.reserve_tokens {
            project.reserve_tokens
        } else {
            global.reserve_tokens
        },
        keep_recent_tokens: if project.keep_recent_tokens != defaults.keep_recent_tokens {
            project.keep_recent_tokens
        } else {
            global.keep_recent_tokens
        },
    }
}

fn merge_retry(global: &RetryConfig, project: &RetryConfig) -> RetryConfig {
    let defaults = RetryConfig::default();
    RetryConfig {
        enabled: if !project.enabled && defaults.enabled {
            false
        } else {
            project.enabled
        },
        max_retries: if project.max_retries != defaults.max_retries {
            project.max_retries
        } else {
            global.max_retries
        },
        base_delay_ms: if project.base_delay_ms != defaults.base_delay_ms {
            project.base_delay_ms
        } else {
            global.base_delay_ms
        },
        max_delay_ms: if project.max_delay_ms != defaults.max_delay_ms {
            project.max_delay_ms
        } else {
            global.max_delay_ms
        },
    }
}

fn merge_string_lists(global: &[String], project: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();

    for value in global.iter().chain(project.iter()) {
        let normalized = value.trim();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.to_owned()) {
            merged.push(normalized.to_owned());
        }
    }

    merged
}

fn merge_bool_with_default(global: bool, project: bool, default: bool) -> bool {
    if project != default { project } else { global }
}

/// Merge package entry lists: project entries override global entries with
/// the same identity (npm name, git repo URL, or resolved local path).
fn merge_package_lists(global: &[PackageEntry], project: &[PackageEntry]) -> Vec<PackageEntry> {
    let mut merged = Vec::new();
    let mut seen_sources = BTreeSet::new();

    // Add global entries first
    for entry in global {
        let source = entry.source().trim();
        if source.is_empty() {
            continue;
        }
        if seen_sources.insert(source.to_owned()) {
            merged.push(entry.clone());
        }
    }

    // Project entries override by identity:
    // Same npm package name, same git repo URL, or same local path
    for entry in project {
        let source = entry.source().trim();
        if source.is_empty() {
            continue;
        }
        let identity = package_entry_identity(source);
        if let Some(pos) = merged
            .iter()
            .position(|e| package_entry_identity(e.source().trim()) == identity)
        {
            merged[pos] = entry.clone();
        } else if seen_sources.insert(source.to_owned()) {
            merged.push(entry.clone());
        }
    }

    merged
}

/// Derive a rough identity from a package source for dedup purposes.
///
/// - npm: strip version → `npm:<name>`
/// - git: strip ref → `git:<url>`
/// - local: use as-is
fn package_entry_identity(source: &str) -> String {
    if let Some(spec) = source.strip_prefix("npm:") {
        let name = if let Some(rest) = spec.strip_prefix('@') {
            // Scoped: @scope/pkg[@version]
            match rest.rfind('@') {
                Some(idx) if rest[..idx].contains('/') => &spec[..idx + 1],
                _ => spec,
            }
        } else {
            spec.rsplit_once('@').map(|(n, _)| n).unwrap_or(spec)
        };
        return format!("npm:{name}");
    }
    if source.starts_with("git:")
        || source.starts_with("https://")
        || source.starts_with("http://")
        || source.starts_with("ssh://")
        || source.starts_with("git://")
    {
        let stripped = source.strip_prefix("git:").unwrap_or(source);
        let (url, _ref) = strip_git_ref_simple(stripped);
        return format!("git:{url}");
    }
    format!("local:{source}")
}

/// Simple git ref stripping for identity purposes.
fn strip_git_ref_simple(spec: &str) -> (&str, Option<&str>) {
    let last_at = spec.rfind('@');
    let Some(index) = last_at else {
        return (spec, None);
    };
    let slash_index = spec.rfind('/').unwrap_or(0);
    let colon_index = spec.rfind(':').unwrap_or(0);
    if index > slash_index.max(colon_index) {
        (&spec[..index], Some(&spec[index + 1..]))
    } else {
        (spec, None)
    }
}

/// Merge model overrides: project models override global models with the
/// same id, and any new project models are appended.
fn merge_optional_vec(
    global: &Option<Vec<ModelOverride>>,
    project: &Option<Vec<ModelOverride>>,
) -> Option<Vec<ModelOverride>> {
    match (global, project) {
        (None, None) => None,
        (Some(g), None) => Some(g.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), Some(p)) => {
            let mut merged = g.clone();
            for pm in p {
                if let Some(pos) = merged.iter().position(|m| m.id == pm.id) {
                    merged[pos] = pm.clone();
                } else {
                    merged.push(pm.clone());
                }
            }
            Some(merged)
        }
    }
}

/// Merge provider overrides similarly.
fn merge_optional_vec_providers(
    global: &Option<Vec<ProviderOverride>>,
    project: &Option<Vec<ProviderOverride>>,
) -> Option<Vec<ProviderOverride>> {
    match (global, project) {
        (None, None) => None,
        (Some(g), None) => Some(g.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), Some(p)) => {
            let mut merged = g.clone();
            for pp in p {
                if let Some(pos) = merged.iter().position(|pr| pr.name == pp.name) {
                    merged[pos] = pp.clone();
                } else {
                    merged.push(pp.clone());
                }
            }
            Some(merged)
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_default() {
        let s = Settings::default();
        assert!(s.compaction.enabled);
        assert_eq!(s.compaction.reserve_tokens, 16384);
        assert_eq!(s.compaction.keep_recent_tokens, 20000);
        assert!(s.default_provider.is_none());
        assert!(s.default_model.is_none());
        assert!(s.extensions.is_empty());
        assert!(s.skills.is_empty());
        assert!(s.prompts.is_empty());
        assert!(s.enable_skill_commands);
    }

    #[test]
    fn test_settings_deserialize() {
        let json = r#"{
            "default_provider": "anthropic",
            "default_model": "sonnet",
            "compaction": {
                "enabled": false,
                "reserve_tokens": 8000
            },
            "skills": ["./skills"],
            "enableSkillCommands": false,
            "models": [
                {
                    "id": "my-model",
                    "provider": "custom",
                    "context_window": 32000
                }
            ]
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.default_provider.as_deref(), Some("anthropic"));
        assert!(!s.compaction.enabled);
        assert_eq!(s.compaction.reserve_tokens, 8000);
        assert_eq!(s.compaction.keep_recent_tokens, 20000); // default
        assert_eq!(s.skills, vec!["./skills"]);
        assert!(!s.enable_skill_commands);
        assert_eq!(s.models.as_ref().unwrap().len(), 1);
        assert_eq!(s.models.as_ref().unwrap()[0].id, "my-model");
    }

    #[test]
    fn test_settings_merge() {
        let global = Settings {
            default_provider: Some("openai".into()),
            default_model: Some("gpt-4o".into()),
            compaction: CompactionConfig {
                enabled: true,
                reserve_tokens: 8192,
                keep_recent_tokens: 20000,
            },
            extensions: vec!["./global-extension.ts".into()],
            skills: vec!["./global-skills".into()],
            enable_skill_commands: true,
            models: Some(vec![ModelOverride {
                id: "custom-1".into(),
                name: Some("Custom 1".into()),
                provider: "custom".into(),
                api: None,
                base_url: None,
                context_window: Some(32000),
                max_tokens: None,
                reasoning: None,
            }]),
            ..Default::default()
        };

        let project = Settings {
            default_provider: Some("anthropic".into()),
            // default_model left as None — global should win
            extensions: vec![
                "./project-extension.ts".into(),
                "./global-extension.ts".into(),
            ],
            prompts: vec!["./project-prompts".into()],
            enable_skill_commands: false,
            models: Some(vec![ModelOverride {
                id: "custom-2".into(),
                name: Some("Custom 2".into()),
                provider: "local".into(),
                api: None,
                base_url: Some("http://localhost:8080".into()),
                context_window: Some(16000),
                max_tokens: None,
                reasoning: None,
            }]),
            ..Default::default()
        };

        let merged = Settings::merge(&global, &project);

        // Project overrides provider
        assert_eq!(merged.default_provider.as_deref(), Some("anthropic"));
        // Global model preserved since project is None
        assert_eq!(merged.default_model.as_deref(), Some("gpt-4o"));
        // Global reserve_tokens preserved (project is default)
        assert_eq!(merged.compaction.reserve_tokens, 8192);
        assert_eq!(
            merged.extensions,
            vec!["./global-extension.ts", "./project-extension.ts"]
        );
        assert_eq!(merged.skills, vec!["./global-skills"]);
        assert_eq!(merged.prompts, vec!["./project-prompts"]);
        assert!(!merged.enable_skill_commands);
        // Both models present
        let models = merged.models.unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "custom-1");
        assert_eq!(models[1].id, "custom-2");
    }

    #[test]
    fn test_settings_merge_model_override() {
        let global = Settings {
            models: Some(vec![ModelOverride {
                id: "my-model".into(),
                name: Some("Old Name".into()),
                provider: "openai".into(),
                api: None,
                base_url: None,
                context_window: Some(32000),
                max_tokens: None,
                reasoning: None,
            }]),
            ..Default::default()
        };

        let project = Settings {
            models: Some(vec![ModelOverride {
                id: "my-model".into(),
                name: Some("New Name".into()),
                provider: "openai".into(),
                api: None,
                base_url: Some("http://localhost".into()),
                context_window: Some(64000),
                max_tokens: None,
                reasoning: None,
            }]),
            ..Default::default()
        };

        let merged = Settings::merge(&global, &project);
        let models = merged.models.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name.as_deref(), Some("New Name"));
        assert_eq!(models[0].context_window, Some(64000));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let s = Settings::load_from_file(Path::new("/nonexistent/path/settings.json"));
        assert!(s.compaction.enabled);
        assert!(s.default_provider.is_none());
    }

    #[test]
    fn test_parse_valid_json() {
        let json = r#"{"default_provider": "anthropic", "compaction": {"enabled": false}}"#;
        let s = Settings::parse(json);
        assert_eq!(s.default_provider.as_deref(), Some("anthropic"));
        assert!(!s.compaction.enabled);
        assert!(s.enable_skill_commands);
    }

    #[test]
    fn test_parse_invalid_json_returns_default() {
        let s = Settings::parse("not valid json");
        assert!(s.compaction.enabled);
        assert!(s.default_provider.is_none());
    }

    #[test]
    fn test_parse_empty_object() {
        let s = Settings::parse("{}");
        assert!(s.compaction.enabled);
        assert_eq!(s.compaction.reserve_tokens, 16384);
    }

    #[test]
    fn test_package_entry_simple_string() {
        let json = r#"{"packages": ["npm:demo", "./local"]}
"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.packages.len(), 2);
        assert_eq!(s.packages[0].source(), "npm:demo");
        assert_eq!(s.packages[1].source(), "./local");
        assert!(s.packages[0].extensions_filter().is_none());
    }

    #[test]
    fn test_package_entry_filtered_object() {
        let json = r#"{
            "packages": [
                "npm:simple",
                {
                    "source": "npm:filtered-pkg",
                    "extensions": ["ext/*.ts", "!ext/legacy.ts"],
                    "skills": [],
                    "prompts": ["prompts/review.md"]
                }
            ]
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.packages.len(), 2);

        assert!(matches!(&s.packages[0], PackageEntry::Simple(src) if src == "npm:simple"));

        let filtered = match &s.packages[1] {
            PackageEntry::Filtered(f) => f,
            _ => panic!("expected filtered entry"),
        };
        assert_eq!(filtered.source, "npm:filtered-pkg");
        assert_eq!(
            filtered.extensions,
            Some(vec!["ext/*.ts".to_string(), "!ext/legacy.ts".to_string()])
        );
        assert_eq!(filtered.skills, Some(vec![])); // empty = load none
        assert_eq!(
            filtered.prompts,
            Some(vec!["prompts/review.md".to_string()])
        );
    }

    #[test]
    fn test_package_merge_dedup_by_identity() {
        let global = Settings {
            packages: vec![PackageEntry::Simple("npm:@demo/pkg@1.0.0".into())],
            ..Settings::default()
        };
        let project = Settings {
            packages: vec![PackageEntry::Simple("npm:@demo/pkg@2.0.0".into())],
            ..Settings::default()
        };
        let merged = Settings::merge(&global, &project);
        assert_eq!(merged.packages.len(), 1);
        assert_eq!(merged.packages[0].source(), "npm:@demo/pkg@2.0.0");
    }

    #[test]
    fn test_package_merge_different_packages_preserved() {
        let global = Settings {
            packages: vec![PackageEntry::Simple("npm:pkg-a".into())],
            ..Settings::default()
        };
        let project = Settings {
            packages: vec![PackageEntry::Simple("npm:pkg-b".into())],
            ..Settings::default()
        };
        let merged = Settings::merge(&global, &project);
        assert_eq!(merged.packages.len(), 2);
        assert_eq!(merged.packages[0].source(), "npm:pkg-a");
        assert_eq!(merged.packages[1].source(), "npm:pkg-b");
    }

    #[test]
    fn test_package_merge_filtered_overrides_simple() {
        let global = Settings {
            packages: vec![PackageEntry::Simple("npm:@demo/pkg@1.0.0".into())],
            ..Settings::default()
        };
        let project = Settings {
            packages: vec![PackageEntry::Filtered(PackageFilter {
                source: "npm:@demo/pkg@2.0.0".into(),
                extensions: Some(vec!["ext/main.ts".into()]),
                skills: Some(vec![]),
                prompts: None,
            })],
            ..Settings::default()
        };
        let merged = Settings::merge(&global, &project);
        assert_eq!(merged.packages.len(), 1);
        assert_eq!(merged.packages[0].source(), "npm:@demo/pkg@2.0.0");
        assert_eq!(
            merged.packages[0].skills_filter(),
            Some([].as_slice())
        );
    }

    #[test]
    fn test_package_entry_roundtrip() {
        let entry = PackageEntry::Filtered(PackageFilter {
            source: "npm:test".into(),
            extensions: Some(vec!["a.ts".into()]),
            skills: None,
            prompts: Some(vec![]),
        });
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: PackageEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, entry);
    }
}
