use bb_core::error::{BbError, BbResult};
use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::text::{lossy_trimmed, truncate_chars_trimmed};

const MIN_VIRTUAL_TIME_BUDGET_MS: u64 = 5_000;
const MAX_VIRTUAL_TIME_BUDGET_MS: u64 = 20_000;

pub(super) fn create_temp_profile_dir() -> BbResult<PathBuf> {
    let dir = env::temp_dir().join(format!("bb-browser-fetch-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir)
        .map_err(|e| BbError::Tool(format!("Failed to create browser profile dir: {e}")))?;
    Ok(dir)
}

pub(super) fn build_browser_args(
    browser: &Path,
    url: &str,
    profile_dir: &Path,
    timeout_secs: f64,
) -> Vec<String> {
    let _ = browser;
    let budget = (timeout_secs * 1_000.0).round().clamp(
        MIN_VIRTUAL_TIME_BUDGET_MS as f64,
        MAX_VIRTUAL_TIME_BUDGET_MS as f64,
    ) as u64;

    let mut args = vec![
        "--headless=new".to_string(),
        "--disable-gpu".to_string(),
        "--disable-dev-shm-usage".to_string(),
        "--hide-scrollbars".to_string(),
        "--mute-audio".to_string(),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
        format!("--user-data-dir={}", profile_dir.display()),
        format!("--virtual-time-budget={budget}"),
        "--dump-dom".to_string(),
        url.to_string(),
    ];

    if should_use_no_sandbox() {
        args.insert(5, "--no-sandbox".to_string());
    }

    args
}

fn should_use_no_sandbox() -> bool {
    if env::var("BB_BROWSER_NO_SANDBOX").as_deref() == Ok("1") {
        return true;
    }
    cfg!(target_os = "linux") && matches!(env::var("USER"), Ok(user) if user == "root")
}

pub(super) async fn run_browser_dump_dom(
    browser: &Path,
    args: &[String],
    timeout_secs: f64,
    cancel: CancellationToken,
) -> BbResult<String> {
    let mut child = Command::new(browser)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            BbError::Tool(format!(
                "Failed to launch browser executable {}: {e}",
                browser.display()
            ))
        })?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();

    let stdout_task = tokio::spawn(async move {
        if let Some(ref mut stdout) = stdout {
            let _ = stdout.read_to_end(&mut stdout_buf).await;
        }
        stdout_buf
    });
    let stderr_task = tokio::spawn(async move {
        if let Some(ref mut stderr) = stderr {
            let _ = stderr.read_to_end(&mut stderr_buf).await;
        }
        stderr_buf
    });

    let status = tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            return Err(BbError::Tool("browser_fetch cancelled".into()));
        }
        _ = tokio::time::sleep(Duration::from_secs_f64(timeout_secs + 5.0)) => {
            let _ = child.kill().await;
            return Err(BbError::Tool("browser_fetch timed out waiting for browser output".into()));
        }
        status = child.wait() => {
            status.map_err(|e| BbError::Tool(format!("browser_fetch failed while waiting for browser: {e}")))?
        }
    };

    let stdout = stdout_task
        .await
        .map_err(|e| BbError::Tool(format!("browser_fetch stdout join error: {e}")))?;
    let stderr = stderr_task
        .await
        .map_err(|e| BbError::Tool(format!("browser_fetch stderr join error: {e}")))?;

    if !status.success() {
        let stderr = lossy_trimmed(&stderr);
        let stdout = lossy_trimmed(&stdout);
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(BbError::Tool(format!(
            "browser_fetch browser process failed: {}",
            truncate_chars_trimmed(&detail, 800)
        )));
    }

    Ok(String::from_utf8_lossy(&stdout).to_string())
}

pub(super) fn resolve_browser_executable() -> Option<PathBuf> {
    if let Ok(path) = env::var("BB_BROWSER") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    for name in [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
        "Google Chrome",
    ] {
        if let Some(path) = find_in_path(name) {
            return Some(path);
        }
    }

    None
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}
