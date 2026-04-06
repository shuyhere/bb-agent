use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use bb_core::agent_session_extensions::{
    PromptTemplateDefinition, PromptTemplateInfo, SkillDefinition, SkillInfo, SourceInfo,
};
use bb_core::config;
use bb_core::settings::Settings;
use serde_json::Value;

use super::ExtensionBootstrap;
use super::packages::ResolvedPackage;

#[derive(Default)]
pub(super) struct DiscoveredResources {
    pub(super) extension_files: Vec<PathBuf>,
    extension_seen: BTreeSet<String>,
    pub(super) skills: Vec<SkillDefinition>,
    skill_seen: BTreeSet<String>,
    pub(super) prompts: Vec<PromptTemplateDefinition>,
    prompt_seen: BTreeSet<String>,
}

#[derive(Default)]
pub(super) struct PackageResources {
    pub(super) extensions: Vec<PathBuf>,
    pub(super) skills: Vec<PathBuf>,
    pub(super) prompts: Vec<PathBuf>,
}

pub(super) fn discover_runtime_resources(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
    packages: &[ResolvedPackage],
) -> Result<DiscoveredResources> {
    let mut discovered = DiscoveredResources::default();

    for root in default_extension_roots(cwd) {
        collect_extension_files_from_entry(
            &root,
            &mut discovered.extension_files,
            &mut discovered.extension_seen,
        );
    }
    for raw_path in &settings.extensions {
        collect_extension_files_from_entry(
            &resolve_input_path(cwd, raw_path),
            &mut discovered.extension_files,
            &mut discovered.extension_seen,
        );
    }
    for path in &bootstrap.paths {
        collect_extension_files_from_entry(
            path,
            &mut discovered.extension_files,
            &mut discovered.extension_seen,
        );
    }

    for root in default_skill_roots(cwd) {
        collect_skills_from_entry(
            &root,
            &mut discovered.skills,
            &mut discovered.skill_seen,
            cwd,
            None,
        );
    }
    for raw_path in &settings.skills {
        collect_skills_from_entry(
            &resolve_input_path(cwd, raw_path),
            &mut discovered.skills,
            &mut discovered.skill_seen,
            cwd,
            None,
        );
    }

    for root in default_prompt_roots(cwd) {
        collect_prompts_from_entry(
            &root,
            &mut discovered.prompts,
            &mut discovered.prompt_seen,
            cwd,
            None,
        );
    }
    for raw_path in &settings.prompts {
        collect_prompts_from_entry(
            &resolve_input_path(cwd, raw_path),
            &mut discovered.prompts,
            &mut discovered.prompt_seen,
            cwd,
            None,
        );
    }

    for resolved in packages {
        collect_package_resources(&mut discovered, cwd, resolved)?;
    }

    Ok(discovered)
}

fn collect_package_resources(
    discovered: &mut DiscoveredResources,
    cwd: &Path,
    resolved: &ResolvedPackage,
) -> Result<()> {
    let package_resources = discover_package_resources(&resolved.dir, cwd)?;
    let ext_filter = resolved.entry.extensions_filter();
    let skill_filter = resolved.entry.skills_filter();
    let prompt_filter = resolved.entry.prompts_filter();

    if !matches!(ext_filter, Some(filter) if filter.is_empty()) {
        let before = discovered.extension_files.len();
        for entry in &package_resources.extensions {
            collect_extension_files_from_entry(
                entry,
                &mut discovered.extension_files,
                &mut discovered.extension_seen,
            );
        }
        if ext_filter.is_some() {
            apply_path_filter(
                &mut discovered.extension_files,
                &mut discovered.extension_seen,
                before,
                &resolved.dir,
                ext_filter,
            );
        }
    }

    if !matches!(skill_filter, Some(filter) if filter.is_empty()) {
        let before = discovered.skills.len();
        for entry in &package_resources.skills {
            collect_skills_from_entry(
                entry,
                &mut discovered.skills,
                &mut discovered.skill_seen,
                cwd,
                Some(&resolved.dir),
            );
        }
        if let Some(patterns) = skill_filter {
            let retained: Vec<_> = discovered.skills[before..]
                .iter()
                .filter(|skill| {
                    filter_matches(
                        Path::new(&skill.info.source_info.path),
                        &resolved.dir,
                        Some(patterns),
                    )
                })
                .cloned()
                .collect();
            discovered.skills.truncate(before);
            discovered.skills.extend(retained);
        }
    }

    if !matches!(prompt_filter, Some(filter) if filter.is_empty()) {
        let before = discovered.prompts.len();
        for entry in &package_resources.prompts {
            collect_prompts_from_entry(
                entry,
                &mut discovered.prompts,
                &mut discovered.prompt_seen,
                cwd,
                Some(&resolved.dir),
            );
        }
        if let Some(patterns) = prompt_filter {
            let retained: Vec<_> = discovered.prompts[before..]
                .iter()
                .filter(|prompt| {
                    filter_matches(
                        Path::new(&prompt.info.source_info.path),
                        &resolved.dir,
                        Some(patterns),
                    )
                })
                .cloned()
                .collect();
            discovered.prompts.truncate(before);
            discovered.prompts.extend(retained);
        }
    }

    Ok(())
}

fn default_extension_roots(cwd: &Path) -> Vec<PathBuf> {
    vec![
        config::global_dir().join("extensions"),
        config::project_dir(cwd).join("extensions"),
    ]
}

fn default_prompt_roots(cwd: &Path) -> Vec<PathBuf> {
    vec![
        config::global_dir().join("prompts"),
        config::project_dir(cwd).join("prompts"),
    ]
}

fn default_skill_roots(cwd: &Path) -> Vec<PathBuf> {
    let mut roots = vec![
        config::global_dir().join("skills"),
        config::project_dir(cwd).join("skills"),
    ];
    if let Some(home) = home_path() {
        roots.push(home.join(".agents").join("skills"));
    }
    for ancestor in cwd.ancestors() {
        roots.push(ancestor.join(".agents").join("skills"));
    }
    roots
}

fn collect_extension_files_from_entry(
    entry: &Path,
    files: &mut Vec<PathBuf>,
    seen: &mut BTreeSet<String>,
) {
    if !entry.exists() {
        return;
    }

    if entry.is_file() {
        if is_extension_file(entry) {
            push_unique_path(files, seen, entry.to_path_buf());
        }
        return;
    }

    let Ok(entries) = fs::read_dir(entry) else {
        return;
    };
    for child in entries.flatten().map(|value| value.path()) {
        if child.is_file() {
            if is_extension_file(&child) {
                push_unique_path(files, seen, child);
            }
        } else if child.is_dir()
            && let Some(index) = resolve_extension_index(&child)
        {
            push_unique_path(files, seen, index);
        }
    }
}

fn collect_skills_from_entry(
    entry: &Path,
    definitions: &mut Vec<SkillDefinition>,
    seen: &mut BTreeSet<String>,
    cwd: &Path,
    package_root: Option<&Path>,
) {
    if !entry.exists() {
        return;
    }

    if entry.is_file() {
        if entry.file_name().and_then(|name| name.to_str()) == Some("SKILL.md")
            || entry.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            push_skill_definition(entry, definitions, seen, cwd, package_root);
        }
        return;
    }

    let is_agents_root = entry
        .to_str()
        .map(|value| value.contains(".agents/skills"))
        .unwrap_or(false);

    let Ok(entries) = fs::read_dir(entry) else {
        return;
    };
    for child in entries.flatten().map(|value| value.path()) {
        if child.is_dir() {
            let skill_file = child.join("SKILL.md");
            if skill_file.is_file() {
                push_skill_definition(&skill_file, definitions, seen, cwd, package_root);
            } else {
                collect_skills_from_entry(&child, definitions, seen, cwd, package_root);
            }
        } else if child.is_file()
            && !is_agents_root
            && child.extension().and_then(|ext| ext.to_str()) == Some("md")
        {
            push_skill_definition(&child, definitions, seen, cwd, package_root);
        }
    }
}

fn collect_prompts_from_entry(
    entry: &Path,
    definitions: &mut Vec<PromptTemplateDefinition>,
    seen: &mut BTreeSet<String>,
    cwd: &Path,
    package_root: Option<&Path>,
) {
    if !entry.exists() {
        return;
    }

    if entry.is_file() {
        if entry.extension().and_then(|ext| ext.to_str()) == Some("md") {
            push_prompt_definition(entry, definitions, seen, cwd, package_root);
        }
        return;
    }

    let Ok(entries) = fs::read_dir(entry) else {
        return;
    };
    for child in entries.flatten().map(|value| value.path()) {
        if child.is_file() && child.extension().and_then(|ext| ext.to_str()) == Some("md") {
            push_prompt_definition(&child, definitions, seen, cwd, package_root);
        }
    }
}

fn push_skill_definition(
    path: &Path,
    definitions: &mut Vec<SkillDefinition>,
    seen: &mut BTreeSet<String>,
    cwd: &Path,
    package_root: Option<&Path>,
) {
    let normalized = normalize_path(path.to_path_buf());
    let key = normalized.display().to_string();
    if !seen.insert(key.clone()) {
        return;
    }

    let Ok(content) = fs::read_to_string(&normalized) else {
        return;
    };
    let metadata = parse_frontmatter(&content);
    let fallback_name = normalized
        .parent()
        .and_then(|parent| parent.file_name())
        .or_else(|| normalized.file_stem())
        .and_then(|value| value.to_str())
        .unwrap_or("skill")
        .to_string();

    definitions.push(SkillDefinition {
        info: SkillInfo {
            name: metadata
                .get("name")
                .cloned()
                .filter(|value| !value.is_empty())
                .unwrap_or(fallback_name),
            description: metadata
                .get("description")
                .cloned()
                .unwrap_or_else(|| first_meaningful_line(&content).unwrap_or_default()),
            source_info: SourceInfo {
                path: key,
                source: resource_source_label(&normalized, cwd, package_root),
            },
        },
        content,
    });
}

fn push_prompt_definition(
    path: &Path,
    definitions: &mut Vec<PromptTemplateDefinition>,
    seen: &mut BTreeSet<String>,
    cwd: &Path,
    package_root: Option<&Path>,
) {
    let normalized = normalize_path(path.to_path_buf());
    let key = normalized.display().to_string();
    if !seen.insert(key.clone()) {
        return;
    }

    let Ok(content) = fs::read_to_string(&normalized) else {
        return;
    };

    definitions.push(PromptTemplateDefinition {
        info: PromptTemplateInfo {
            name: normalized
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("prompt")
                .to_string(),
            description: first_meaningful_line(&content).unwrap_or_default(),
            source_info: SourceInfo {
                path: key,
                source: resource_source_label(&normalized, cwd, package_root),
            },
        },
        content,
    });
}

fn resource_source_label(path: &Path, cwd: &Path, package_root: Option<&Path>) -> String {
    if let Some(root) = package_root {
        let name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("package");
        return format!("package:{name}");
    }
    if path.starts_with(config::global_dir()) {
        return "settings:global".to_string();
    }
    if path.starts_with(config::project_dir(cwd)) || path.starts_with(cwd) {
        return "settings:project".to_string();
    }
    "settings:external".to_string()
}

pub(super) fn parse_frontmatter(content: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return values;
    }

    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    values
}

fn first_meaningful_line(content: &str) -> Option<String> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "---" && !line.starts_with('#'))
        .map(ToOwned::to_owned)
}

pub(super) fn resolve_input_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        normalize_path(path.to_path_buf())
    } else {
        normalize_path(base_dir.join(path))
    }
}

pub(super) fn normalize_path(path: PathBuf) -> PathBuf {
    fs::canonicalize(&path).unwrap_or(path)
}

fn push_unique_path(paths: &mut Vec<PathBuf>, seen: &mut BTreeSet<String>, path: PathBuf) {
    let normalized = normalize_path(path);
    let key = normalized.display().to_string();
    if seen.insert(key) {
        paths.push(normalized);
    }
}

fn resolve_extension_index(dir: &Path) -> Option<PathBuf> {
    ["index.ts", "index.js", "index.mjs", "index.cjs"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

fn is_extension_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ts" | "js" | "mjs" | "cjs")
    )
}

fn home_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub(super) fn discover_package_resources(
    package_dir: &Path,
    cwd: &Path,
) -> Result<PackageResources> {
    if !package_dir.exists() {
        return Ok(PackageResources::default());
    }

    let manifest = package_dir.join("package.json");
    if manifest.is_file() {
        let package_json: Value = serde_json::from_str(
            &fs::read_to_string(&manifest)
                .with_context(|| format!("read {}", manifest.display()))?,
        )
        .with_context(|| format!("parse {}", manifest.display()))?;

        if let Some(bb) = package_json.get("bb").and_then(Value::as_object) {
            return Ok(PackageResources {
                extensions: manifest_entries(package_dir, bb.get("extensions")),
                skills: manifest_entries(package_dir, bb.get("skills")),
                prompts: manifest_entries(package_dir, bb.get("prompts")),
            });
        }
    }

    let mut resources = PackageResources::default();
    for (dir_name, target) in [
        ("extensions", &mut resources.extensions),
        ("skills", &mut resources.skills),
        ("prompts", &mut resources.prompts),
    ] {
        let path = package_dir.join(dir_name);
        if path.exists() {
            target.push(normalize_path(path));
        }
    }

    if resources.extensions.is_empty()
        && resources.skills.is_empty()
        && resources.prompts.is_empty()
        && package_dir.starts_with(cwd)
        && (package_dir.is_dir() || package_dir.is_file())
    {
        resources
            .extensions
            .push(normalize_path(package_dir.to_path_buf()));
    }

    Ok(resources)
}

pub(super) fn filter_matches(path: &Path, package_root: &Path, filter: Option<&[String]>) -> bool {
    let Some(patterns) = filter else {
        return true;
    };
    if patterns.is_empty() {
        return false;
    }

    let relative = path
        .strip_prefix(package_root)
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.display().to_string());

    let mut included = false;
    let mut force_excluded = false;
    let mut force_included = false;

    for pattern in patterns {
        if let Some(exact) = pattern.strip_prefix('+') {
            if relative == exact || path.ends_with(exact) {
                force_included = true;
            }
        } else if let Some(exact) = pattern.strip_prefix('-') {
            if relative == exact || path.ends_with(exact) {
                force_excluded = true;
            }
        } else if let Some(glob_pattern) = pattern.strip_prefix('!') {
            if glob_pattern_matches(glob_pattern, &relative) {
                force_excluded = true;
            }
        } else if glob_pattern_matches(pattern, &relative) {
            included = true;
        }
    }

    if force_excluded && !force_included {
        return false;
    }
    if force_included {
        return true;
    }
    included
}

fn glob_pattern_matches(pattern: &str, relative: &str) -> bool {
    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    glob::Pattern::new(pattern)
        .map(|compiled| compiled.matches_with(relative, options))
        .unwrap_or(false)
}

fn apply_path_filter(
    paths: &mut Vec<PathBuf>,
    seen: &mut BTreeSet<String>,
    start: usize,
    package_root: &Path,
    filter: Option<&[String]>,
) {
    let retained: Vec<PathBuf> = paths[start..]
        .iter()
        .filter(|path| filter_matches(path, package_root, filter))
        .cloned()
        .collect();
    for removed in &paths[start..] {
        if !retained
            .iter()
            .any(|retained_path| retained_path == removed)
        {
            seen.remove(&removed.display().to_string());
        }
    }
    paths.truncate(start);
    paths.extend(retained);
}

fn manifest_entries(package_dir: &Path, value: Option<&Value>) -> Vec<PathBuf> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(|entry| normalize_path(package_dir.join(entry)))
                .collect()
        })
        .unwrap_or_default()
}
