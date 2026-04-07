use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use bb_core::settings::Settings;
use bb_tui::fullscreen::{FullscreenCommand, FullscreenNoteLevel};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

const DEFAULT_NPM_PACKAGE: Option<&str> = Some("@shuyhere/bb-agent");
const DEFAULT_CHANGELOG_URL: Option<&str> = Some("https://github.com/shuyhere/bb-agent/releases");
const DEFAULT_INSTALL_COMMAND: Option<&str> = None;
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, PartialEq, Eq)]
struct UpdateCheckConfig {
    package_name: String,
    current_version: String,
    install_command: String,
    changelog_url: Option<String>,
    cache_ttl: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UpdateNotice {
    pub latest_version: String,
    pub install_command: String,
    pub changelog_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NpmLatestResponse {
    version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateCheckOutcome {
    Disabled,
    UpToDate,
    UpdateAvailable(UpdateNotice),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct UpdateCheckCache {
    package_name: String,
    current_version: String,
    checked_at_unix_secs: u64,
    notice: Option<UpdateNotice>,
}

pub(crate) fn spawn_update_check_notice_task(
    command_tx: mpsc::UnboundedSender<FullscreenCommand>,
    cwd: PathBuf,
) {
    tokio::spawn(async move {
        match check_for_updates(false, &cwd).await {
            Ok(UpdateCheckOutcome::UpdateAvailable(notice)) => {
                let _ = command_tx.send(FullscreenCommand::PushNote {
                    level: FullscreenNoteLevel::Status,
                    text: build_update_available_note(&notice),
                });
            }
            Ok(UpdateCheckOutcome::Disabled | UpdateCheckOutcome::UpToDate) => {}
            Err(err) => tracing::debug!("update check skipped: {err}"),
        }
    });
}

pub(crate) async fn check_for_updates(
    force_refresh: bool,
    cwd: &Path,
) -> anyhow::Result<UpdateCheckOutcome> {
    let Some(config) = load_config(cwd) else {
        return Ok(UpdateCheckOutcome::Disabled);
    };

    if !force_refresh && let Some(cached) = load_cached_outcome(&config)? {
        return Ok(cached);
    }

    let notice = fetch_update_notice(&config).await?;
    store_cached_outcome(&config, notice.as_ref())?;
    Ok(match notice {
        Some(notice) => UpdateCheckOutcome::UpdateAvailable(notice),
        None => UpdateCheckOutcome::UpToDate,
    })
}

fn detect_install_command(package_name: &str) -> String {
    if let Ok(cmd) = std::env::var("BB_UPDATE_CHECK_INSTALL")
        && !cmd.trim().is_empty()
    {
        return cmd;
    }
    if std::env::var("BB_NPM_WRAPPER_ACTIVE").ok().as_deref() == Some("1") {
        return format!("npm install -g {package_name}@latest");
    }
    if let Ok(exe) = std::env::current_exe() {
        let exe = exe.display().to_string().to_ascii_lowercase();
        if exe.contains("node_modules") || exe.contains("homebrew") || exe.contains("npm") {
            return format!("npm install -g {package_name}@latest");
        }
        if exe.contains(".cargo") || exe.contains("cargo") {
            return "cargo install --git https://github.com/shuyhere/bb-agent.git bb-cli --force"
                .to_string();
        }
    }
    DEFAULT_INSTALL_COMMAND
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("npm install -g {package_name}@latest"))
}

fn load_config(cwd: &Path) -> Option<UpdateCheckConfig> {
    let settings = Settings::load_merged(cwd);
    if !settings.update_check.enabled {
        return None;
    }

    let package_name = std::env::var("BB_UPDATE_CHECK_PACKAGE")
        .ok()
        .or_else(|| DEFAULT_NPM_PACKAGE.map(ToString::to_string))?;
    let install_command = detect_install_command(&package_name);
    let changelog_url = std::env::var("BB_UPDATE_CHECK_CHANGELOG")
        .ok()
        .or_else(|| DEFAULT_CHANGELOG_URL.map(ToString::to_string));

    let ttl_hours = std::env::var("BB_UPDATE_CHECK_TTL_HOURS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(settings.update_check.ttl_hours);

    Some(UpdateCheckConfig {
        package_name,
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        install_command,
        changelog_url,
        cache_ttl: Duration::from_secs(ttl_hours.saturating_mul(60 * 60)),
    })
}

fn load_cached_outcome(config: &UpdateCheckConfig) -> anyhow::Result<Option<UpdateCheckOutcome>> {
    load_cached_outcome_from_path(config, &cache_file_path())
}

fn load_cached_outcome_from_path(
    config: &UpdateCheckConfig,
    path: &std::path::Path,
) -> anyhow::Result<Option<UpdateCheckOutcome>> {
    let Ok(content) = fs::read_to_string(path) else {
        return Ok(None);
    };
    let Ok(cache) = serde_json::from_str::<UpdateCheckCache>(&content) else {
        return Ok(None);
    };
    if cache.package_name != config.package_name || cache.current_version != config.current_version
    {
        return Ok(None);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now.saturating_sub(cache.checked_at_unix_secs) > config.cache_ttl.as_secs() {
        return Ok(None);
    }

    Ok(Some(match cache.notice {
        Some(notice) => UpdateCheckOutcome::UpdateAvailable(notice),
        None => UpdateCheckOutcome::UpToDate,
    }))
}

fn store_cached_outcome(
    config: &UpdateCheckConfig,
    notice: Option<&UpdateNotice>,
) -> anyhow::Result<()> {
    store_cached_outcome_to_path(config, notice, &cache_file_path())
}

fn store_cached_outcome_to_path(
    config: &UpdateCheckConfig,
    notice: Option<&UpdateNotice>,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let cache = UpdateCheckCache {
        package_name: config.package_name.clone(),
        current_version: config.current_version.clone(),
        checked_at_unix_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        notice: notice.cloned(),
    };
    fs::write(path, serde_json::to_vec_pretty(&cache)?)?;
    Ok(())
}

fn cache_file_path() -> PathBuf {
    if let Ok(path) = std::env::var("BB_UPDATE_CHECK_CACHE_PATH") {
        return PathBuf::from(path);
    }
    bb_core::config::global_dir().join("update-check.json")
}

async fn fetch_update_notice(config: &UpdateCheckConfig) -> anyhow::Result<Option<UpdateNotice>> {
    let encoded_package = encode_registry_package_name(&config.package_name);
    let url = format!("https://registry.npmjs.org/{encoded_package}/latest");
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    let response = client.get(url).send().await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let response = response.error_for_status()?;
    let latest: NpmLatestResponse = response.json().await?;
    if is_newer_version(&latest.version, &config.current_version) {
        Ok(Some(UpdateNotice {
            latest_version: latest.version,
            install_command: config.install_command.clone(),
            changelog_url: config.changelog_url.clone(),
        }))
    } else {
        Ok(None)
    }
}

fn encode_registry_package_name(package_name: &str) -> String {
    package_name.replace('/', "%2F")
}

fn parse_version_core(version: &str) -> Vec<u64> {
    let core = version
        .split_once('-')
        .map(|(core, _)| core)
        .unwrap_or(version);
    let core = core.split_once('+').map(|(core, _)| core).unwrap_or(core);
    core.split('.')
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

fn is_prerelease(version: &str) -> bool {
    version.contains('-')
}

fn is_newer_version(candidate: &str, current: &str) -> bool {
    let lhs = parse_version_core(candidate);
    let rhs = parse_version_core(current);
    let len = lhs.len().max(rhs.len());

    for index in 0..len {
        let left = lhs.get(index).copied().unwrap_or(0);
        let right = rhs.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }

    !is_prerelease(candidate) && is_prerelease(current)
}

pub(crate) fn build_update_available_note(notice: &UpdateNotice) -> String {
    let mut lines = vec![format!(
        "bb update available: {} • use {}",
        notice.latest_version, notice.install_command
    )];
    if let Some(changelog_url) = &notice.changelog_url {
        lines.push(format!("release notes: {changelog_url}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        UpdateCheckOutcome, UpdateNotice, build_update_available_note, is_newer_version,
        load_cached_outcome_from_path, store_cached_outcome_to_path,
    };

    #[test]
    fn compares_semver_like_versions() {
        assert!(is_newer_version("0.65.0", "0.64.9"));
        assert!(is_newer_version("1.0.0", "0.99.0"));
        assert!(!is_newer_version("0.65.0", "0.65.0"));
        assert!(!is_newer_version("0.64.9", "0.65.0"));
        assert!(is_newer_version("0.65.0", "0.65.0-beta.1"));
    }

    #[test]
    fn formats_update_available_note() {
        let text = build_update_available_note(&UpdateNotice {
            latest_version: "0.65.0".to_string(),
            install_command: "npm install -g bb-agent".to_string(),
            changelog_url: Some("https://example.com/bb-agent/changelog".to_string()),
        });

        assert!(text.contains("bb update available: 0.65.0"));
        assert!(text.contains("npm install -g bb-agent"));
        assert!(text.contains("release notes: https://example.com/bb-agent/changelog"));
    }

    #[test]
    fn cache_round_trip_preserves_available_update() {
        let dir = tempfile::tempdir().unwrap();
        let cache_path = dir.path().join("update-check.json");
        let config = super::UpdateCheckConfig {
            package_name: "npm:demo".to_string(),
            current_version: "0.1.0".to_string(),
            install_command: "npm install -g demo".to_string(),
            changelog_url: None,
            cache_ttl: Duration::from_secs(60 * 60 * 24),
        };
        let notice = UpdateNotice {
            latest_version: "0.2.0".to_string(),
            install_command: "npm install -g demo".to_string(),
            changelog_url: None,
        };

        store_cached_outcome_to_path(&config, Some(&notice), &cache_path).unwrap();
        let loaded = load_cached_outcome_from_path(&config, &cache_path).unwrap();
        assert_eq!(loaded, Some(UpdateCheckOutcome::UpdateAvailable(notice)));
    }
}
