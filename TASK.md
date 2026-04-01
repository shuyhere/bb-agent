# Sprint 3: Settings Manager + Model Resolver

You are working in a git worktree at `/tmp/bb-worktrees/s3-settings-models/`.
This is the BB-Agent project — a Rust coding agent. Read `BLUEPRINT.md` and `PLAN.md` for context.

## Your task

Build a layered settings system and fuzzy model resolver.

### 1. Create `crates/core/src/settings.rs` (~300 lines)

Layered settings: global (`~/.bb-agent/settings.json`) merged with project (`.bb-agent/settings.json`).

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub compaction: CompactionConfig,
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub default_thinking: Option<String>,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub models: Option<Vec<ModelOverride>>,
    #[serde(default)]
    pub providers: Option<Vec<ProviderOverride>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CompactionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_reserve")]
    pub reserve_tokens: u64,
    #[serde(default = "default_keep")]
    pub keep_recent_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelOverride {
    pub id: String,
    pub name: Option<String>,
    pub provider: String,
    pub api: Option<String>,            // "openai-completions", "anthropic-messages", etc.
    pub base_url: Option<String>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub reasoning: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderOverride {
    pub name: String,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,    // env var name for API key
    pub api: Option<String>,
    pub headers: Option<std::collections::HashMap<String, String>>,
}

impl Settings {
    pub fn load_global() -> Self;
    pub fn load_project(cwd: &Path) -> Self;
    pub fn merge(global: &Self, project: &Self) -> Self;
}
```

The merge logic: project values override global when present (non-None/non-default).

### 2. Create `crates/provider/src/resolver.rs` (~200 lines)

Fuzzy model matching so users can type `--model sonnet` instead of full IDs.

```rust
/// Parse a model argument. Supports:
///   "gpt-4o"                  → (None, "gpt-4o", None)
///   "openai/gpt-4o"           → (Some("openai"), "gpt-4o", None)
///   "sonnet:high"             → (None, "sonnet", Some("high"))
///   "anthropic/sonnet:high"   → (Some("anthropic"), "sonnet", Some("high"))
pub fn parse_model_arg(input: &str) -> (Option<String>, String, Option<String>);

/// Fuzzy-match a model pattern against the registry.
/// "sonnet" matches "claude-sonnet-4-20250514"
/// "gpt4o" matches "gpt-4o"
/// Returns best match or None.
pub fn fuzzy_find_model(registry: &ModelRegistry, pattern: &str, provider: Option<&str>) -> Option<Model>;

/// Simple fuzzy matching score.
/// Returns 0 if no match, higher = better match.
pub fn fuzzy_score(pattern: &str, text: &str) -> u32;
```

Implementation:
- Lowercase both pattern and candidate
- Try exact match first
- Try substring match (`candidate.contains(pattern)`)
- Try fuzzy: check if all pattern chars appear in order in candidate
- Score: prefer shorter candidates, prefer matches at word boundaries

### 3. Modify `crates/provider/src/registry.rs`

Add support for loading custom models from settings:

```rust
impl ModelRegistry {
    pub fn new() -> Self;  // existing, loads builtins

    /// Load additional models from settings.
    pub fn load_custom_models(&mut self, settings: &Settings);

    /// Load additional models from a JSON file (models.json).
    pub fn load_from_file(&mut self, path: &Path);

    /// Find model with fuzzy matching.
    pub fn find_fuzzy(&self, pattern: &str, provider: Option<&str>) -> Option<&Model>;
}
```

### 4. Modify `crates/cli/src/run.rs`

Update to use the new settings system:
- Load global + project settings
- Apply default model/provider from settings
- Use `find_fuzzy()` for `--model` resolution
- Pass compaction settings to session

### 5. Tests

- `test_settings_merge` — project overrides global
- `test_parse_model_arg` — all format variants
- `test_fuzzy_score` — matching logic
- `test_fuzzy_find_model` — finds correct model
- `test_load_custom_models` — custom model appears in registry

## Build and test

```bash
cd /tmp/bb-worktrees/s3-settings-models
cargo build
cargo test
```

Make sure ALL existing tests still pass. Then commit:
```bash
git add -A && git commit -m "S3: settings manager + fuzzy model resolver"
```
