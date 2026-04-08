use std::collections::BTreeSet;

use super::{
    CompactionConfig, ModelOverride, PackageEntry, ProviderOverride, RetryConfig, Settings,
    UpdateCheckSettings,
};

pub(super) fn merge_settings(global: &Settings, project: &Settings) -> Settings {
    Settings {
        execution_mode: project.execution_mode.or(global.execution_mode),
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
            crate::settings_defaults::default_enable_skill_commands(),
        ),
        models: merge_optional_vec(&global.models, &project.models),
        providers: merge_optional_vec_providers(&global.providers, &project.providers),
        color_theme: project
            .color_theme
            .clone()
            .or_else(|| global.color_theme.clone()),
        compatibility_mode: merge_bool_with_default(
            global.compatibility_mode,
            project.compatibility_mode,
            false,
        ),
        update_check: merge_update_check(&global.update_check, &project.update_check),
    }
}

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

fn merge_update_check(
    global: &UpdateCheckSettings,
    project: &UpdateCheckSettings,
) -> UpdateCheckSettings {
    let defaults = UpdateCheckSettings::default();
    UpdateCheckSettings {
        enabled: if !project.enabled && defaults.enabled {
            false
        } else {
            project.enabled
        },
        ttl_hours: if project.ttl_hours != defaults.ttl_hours {
            project.ttl_hours
        } else {
            global.ttl_hours
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
    merge_value_with_default(global, project, default)
}

fn merge_value_with_default<T>(global: T, project: T, default: T) -> T
where
    T: Copy + PartialEq,
{
    if project != default { project } else { global }
}

/// Merge package entry lists: project entries override global entries with
/// the same identity (npm name, git repo URL, or resolved local path).
fn merge_package_lists(global: &[PackageEntry], project: &[PackageEntry]) -> Vec<PackageEntry> {
    let mut merged = Vec::new();
    let mut seen_sources = BTreeSet::new();

    for entry in global {
        let source = entry.source().trim();
        if source.is_empty() {
            continue;
        }
        if seen_sources.insert(source.to_owned()) {
            merged.push(entry.clone());
        }
    }

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
