use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use bb_core::config;
use bb_core::settings::{PackageEntry, Settings};
use sha2::{Digest, Sha256};

use super::ExtensionBootstrap;
use super::discovery::{normalize_path, resolve_input_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackageSource<'a> {
    LocalPath(&'a str),
    Npm(&'a str),
    Git(&'a str),
}

impl<'a> PackageSource<'a> {
    pub(crate) fn parse(source: &'a str) -> Self {
        if let Some(spec) = source.strip_prefix("npm:") {
            Self::Npm(spec)
        } else if source.starts_with("git:")
            || source.starts_with("https://")
            || source.starts_with("http://")
            || source.starts_with("ssh://")
            || source.starts_with("git://")
        {
            Self::Git(source)
        } else {
            Self::LocalPath(source)
        }
    }

    fn resolve_directory(self, cwd: &Path) -> Result<PathBuf> {
        match self {
            Self::LocalPath(path) => Ok(resolve_input_path(cwd, path)),
            Self::Npm(spec) => resolve_npm_package_dir(spec, cwd),
            Self::Git(spec) => Ok(git_package_install_dir(spec, cwd)),
        }
    }

    fn install(self, scope: SettingsScope, cwd: &Path) -> Result<()> {
        match self {
            Self::LocalPath(path) => {
                let resolved = resolve_input_path(cwd, path);
                if !resolved.exists() {
                    bail!("package path does not exist: {}", resolved.display());
                }
                Ok(())
            }
            Self::Npm(spec) => install_npm_package(spec, scope, cwd),
            Self::Git(spec) => install_git_package(spec, scope, cwd),
        }
    }

    fn identity(self, cwd: &Path) -> Result<String> {
        match self {
            Self::LocalPath(path) => {
                Ok(format!("local:{}", resolve_input_path(cwd, path).display()))
            }
            Self::Npm(spec) => Ok(format!("npm:{}", npm_package_name(spec)?)),
            Self::Git(spec) => Ok(format!("git:{}", git_repo_url(spec))),
        }
    }

    fn is_pinned(self) -> bool {
        match self {
            Self::LocalPath(_) => false,
            Self::Npm(spec) => npm_package_name(spec)
                .map(|name| name != spec)
                .unwrap_or(false),
            Self::Git(spec) => git_ref(spec).is_some(),
        }
    }

    fn resolved_install_root(self, cwd: &Path) -> Option<PathBuf> {
        match self {
            Self::LocalPath(_) => None,
            Self::Npm(spec) => Some(resolve_install_root("npm", spec, cwd)),
            Self::Git(spec) => Some(resolve_install_root("git", spec, cwd)),
        }
    }
}

pub(crate) fn is_package_source(value: &str) -> bool {
    matches!(
        PackageSource::parse(value),
        PackageSource::Npm(_) | PackageSource::Git(_)
    )
}

#[cfg(test)]
pub(crate) fn classify_package_source(source: &str) -> PackageSource<'_> {
    PackageSource::parse(source)
}

/// A resolved package directory together with its optional filter.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedPackage {
    pub(crate) dir: PathBuf,
    pub(crate) entry: PackageEntry,
}

/// Check all packages in the merged settings and auto-install any whose
/// install directories do not yet exist. This preserves the historical
/// behavior where packages listed in settings are transparently installed on
/// first launch.
pub(crate) fn auto_install_missing_packages(cwd: &Path, settings: &Settings) {
    for entry in &settings.packages {
        let source = entry.source().trim();
        if source.is_empty() {
            continue;
        }

        let package = PackageSource::parse(source);
        let Some(root) = package.resolved_install_root(cwd) else {
            continue;
        };
        if root.exists() {
            continue;
        }

        tracing::info!("auto-installing missing package {source}");
        if let Err(err) = package.install(SettingsScope::Global, cwd) {
            tracing::warn!("failed to auto-install {source}: {err}");
        }
    }
}

pub(crate) fn install_package(source: &str, scope: SettingsScope, cwd: &Path) -> Result<()> {
    PackageSource::parse(source).install(scope, cwd)?;

    let mut settings = load_settings_for_scope(scope, cwd);
    append_unique_package(
        &mut settings.packages,
        PackageEntry::Simple(source.to_string()),
        cwd,
    )?;
    save_settings_for_scope(scope, cwd, &settings)
}

pub(crate) fn remove_package(source: &str, scope: SettingsScope, cwd: &Path) -> Result<bool> {
    let mut settings = load_settings_for_scope(scope, cwd);
    let target_identity = package_identity(source, cwd)?;
    let before = settings.packages.len();
    settings.packages.retain(|entry| {
        package_identity(entry.source(), cwd).ok().as_deref() != Some(target_identity.as_str())
    });
    let removed = before != settings.packages.len();
    if removed {
        save_settings_for_scope(scope, cwd, &settings)?;
    }
    Ok(removed)
}

pub(crate) fn list_packages(scope: Option<SettingsScope>, cwd: &Path) -> Vec<String> {
    let entries = match scope {
        Some(scope) => load_settings_for_scope(scope, cwd).packages,
        None => {
            let global = load_settings_for_scope(SettingsScope::Global, cwd).packages;
            let project = load_settings_for_scope(SettingsScope::Project, cwd).packages;
            Settings::merge(
                &Settings {
                    packages: global,
                    ..Settings::default()
                },
                &Settings {
                    packages: project,
                    ..Settings::default()
                },
            )
            .packages
        }
    };
    entries
        .iter()
        .map(|entry| entry.source().to_string())
        .collect()
}

pub(crate) fn update_packages(scope: Option<SettingsScope>, cwd: &Path) -> Result<Vec<String>> {
    let effective_scope = scope.unwrap_or(SettingsScope::Global);
    let mut updated = Vec::new();

    for package in list_packages(scope, cwd) {
        let parsed = PackageSource::parse(&package);
        if parsed.is_pinned() {
            continue;
        }
        parsed.install(effective_scope, cwd)?;
        updated.push(package);
    }

    Ok(updated)
}

pub(crate) fn resolve_package_directories(
    cwd: &Path,
    settings: &Settings,
    bootstrap: &ExtensionBootstrap,
) -> Result<Vec<ResolvedPackage>> {
    let mut resolved = Vec::new();
    let mut seen = BTreeSet::new();

    for entry in &settings.packages {
        let path = resolve_package_directory(cwd, entry.source())?;
        let key = normalize_path(path.clone()).display().to_string();
        if seen.insert(key) {
            resolved.push(ResolvedPackage {
                dir: path,
                entry: entry.clone(),
            });
        }
    }

    for source in &bootstrap.package_sources {
        let path = resolve_package_directory(cwd, source)?;
        let key = normalize_path(path.clone()).display().to_string();
        if seen.insert(key) {
            resolved.push(ResolvedPackage {
                dir: path,
                entry: PackageEntry::Simple(source.clone()),
            });
        }
    }

    Ok(resolved)
}

pub(crate) fn resolve_package_directory(cwd: &Path, source: &str) -> Result<PathBuf> {
    PackageSource::parse(source).resolve_directory(cwd)
}

pub(crate) fn npm_package_name(spec: &str) -> Result<String> {
    if spec.is_empty() {
        bail!("empty npm package spec");
    }
    if let Some(rest) = spec.strip_prefix('@') {
        let second_at = rest.rfind('@');
        let candidate = match second_at {
            Some(index) if rest[..index].contains('/') => &spec[..index + 1],
            _ => spec,
        };
        Ok(candidate.to_string())
    } else {
        Ok(spec
            .rsplit_once('@')
            .map(|(name, _)| name)
            .unwrap_or(spec)
            .to_string())
    }
}

fn load_settings_for_scope(scope: SettingsScope, cwd: &Path) -> Settings {
    match scope {
        SettingsScope::Global => Settings::load_global(),
        SettingsScope::Project => Settings::load_project(cwd),
    }
}

fn save_settings_for_scope(scope: SettingsScope, cwd: &Path, settings: &Settings) -> Result<()> {
    match scope {
        SettingsScope::Global => settings.save_global().map_err(Into::into),
        SettingsScope::Project => settings.save_project(cwd).map_err(Into::into),
    }
}

fn append_unique_package(
    values: &mut Vec<PackageEntry>,
    value: PackageEntry,
    cwd: &Path,
) -> Result<()> {
    let identity = package_identity(value.source(), cwd)?;
    if let Some(existing_index) = values.iter().position(|existing| {
        package_identity(existing.source(), cwd).ok().as_deref() == Some(identity.as_str())
    }) {
        values[existing_index] = value;
    } else {
        values.push(value);
    }
    Ok(())
}

fn package_identity(source: &str, cwd: &Path) -> Result<String> {
    PackageSource::parse(source).identity(cwd)
}

/// Scope-aware install root for npm/git packages.
///
/// - Global: `~/.bb-agent/<kind>/<hash>`
/// - Project: `<project-root>/.bb-agent/<kind>/<hash>` when a project root is detected,
///   otherwise `<cwd>/.bb-agent/<kind>/<hash>`
pub(crate) fn package_install_root(
    kind: &str,
    spec: &str,
    scope: SettingsScope,
    cwd: &Path,
) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(spec.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    match scope {
        SettingsScope::Global => config::global_dir().join(kind).join(hash),
        SettingsScope::Project => config::project_dir(cwd).join(kind).join(hash),
    }
}

/// Resolve install root: check project-local first, then global.
fn resolve_install_root(kind: &str, spec: &str, cwd: &Path) -> PathBuf {
    let project = package_install_root(kind, spec, SettingsScope::Project, cwd);
    if project.exists() {
        return project;
    }
    package_install_root(kind, spec, SettingsScope::Global, cwd)
}

fn install_npm_package(spec: &str, scope: SettingsScope, cwd: &Path) -> Result<()> {
    let install_root = package_install_root("npm", spec, scope, cwd);
    fs::create_dir_all(&install_root)?;
    run_command(
        Command::new("npm")
            .arg("install")
            .arg(spec)
            .current_dir(&install_root),
        &format!("npm install {spec}"),
    )
}

fn resolve_npm_package_dir(spec: &str, cwd: &Path) -> Result<PathBuf> {
    let install_root = resolve_install_root("npm", spec, cwd);
    let package_name = npm_package_name(spec)?;
    Ok(install_root.join("node_modules").join(package_name))
}

fn install_git_package(spec: &str, scope: SettingsScope, cwd: &Path) -> Result<()> {
    let install_root = package_install_root("git", spec, scope, cwd);
    let repo = git_repo_url(spec);
    if install_root.exists() {
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&install_root)
                .arg("pull")
                .arg("--ff-only"),
            &format!("git pull {}", install_root.display()),
        )?;
    } else {
        if let Some(parent) = install_root.parent() {
            fs::create_dir_all(parent)?;
        }
        run_command(
            Command::new("git")
                .arg("clone")
                .arg(repo)
                .arg(&install_root),
            &format!("git clone {repo}"),
        )?;
    }

    if let Some(reference) = git_ref(spec) {
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&install_root)
                .arg("checkout")
                .arg(reference),
            &format!("git checkout {reference}"),
        )?;
    }

    if install_root.join("package.json").is_file() {
        run_command(
            Command::new("npm")
                .arg("install")
                .current_dir(&install_root),
            &format!("npm install in {}", install_root.display()),
        )?;
    }

    Ok(())
}

fn git_package_install_dir(spec: &str, cwd: &Path) -> PathBuf {
    resolve_install_root("git", spec, cwd)
}

fn git_repo_url(spec: &str) -> &str {
    let stripped = spec.strip_prefix("git:").unwrap_or(spec);
    strip_git_ref(stripped).0
}

fn git_ref(spec: &str) -> Option<&str> {
    let stripped = spec.strip_prefix("git:").unwrap_or(spec);
    strip_git_ref(stripped).1
}

fn strip_git_ref(spec: &str) -> (&str, Option<&str>) {
    let Some(index) = spec.rfind('@') else {
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

fn run_command(command: &mut Command, description: &str) -> Result<()> {
    let status = command.status().with_context(|| description.to_string())?;
    if !status.success() {
        bail!("{description} failed with status {status}");
    }
    Ok(())
}
