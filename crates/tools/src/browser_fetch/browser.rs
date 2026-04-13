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

pub(super) fn missing_browser_error_message() -> String {
    let mut lines = vec![
        "No supported Chrome/Chromium browser executable found.".to_string(),
        "Set BB_BROWSER to a Chrome/Chromium binary path or install Google Chrome / Chromium.".to_string(),
    ];

    if let Ok(configured) = env::var("BB_BROWSER") {
        let trimmed = configured.trim();
        if !trimmed.is_empty() {
            lines.push(format!("BB_BROWSER is currently set to: {trimmed}"));
        }
    }

    lines.push(format!(
        "Checked PATH/common candidates: {}",
        browser_candidate_labels().join(", ")
    ));

    if cfg!(target_os = "linux") {
        lines.push(
            "Linux hint: install Chromium/Chrome or set BB_BROWSER=/path/to/chrome (for Ubuntu snap installs, /snap/bin/chromium is a common path).".to_string(),
        );
    } else if cfg!(target_os = "macos") {
        lines.push(
            "macOS hint: Google Chrome is commonly at /Applications/Google Chrome.app/Contents/MacOS/Google Chrome .".to_string(),
        );
    } else if cfg!(target_os = "windows") {
        lines.push(
            "Windows hint: set BB_BROWSER to chrome.exe/msedge.exe if it is not on PATH.".to_string(),
        );
    }

    lines.join(" ")
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

    for name in browser_candidate_names() {
        if let Some(path) = find_in_path(name) {
            return Some(path);
        }
    }

    for path in common_browser_paths() {
        if path.exists() {
            return Some(path);
        }
    }

    None
}

fn browser_candidate_names() -> &'static [&'static str] {
    &[
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "chrome",
        "Google Chrome",
        "microsoft-edge",
        "microsoft-edge-stable",
        "msedge",
    ]
}

fn browser_candidate_labels() -> Vec<String> {
    let mut labels: Vec<String> = browser_candidate_names()
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    labels.extend(
        common_browser_paths()
            .into_iter()
            .map(|path| path.display().to_string()),
    );
    labels
}

fn common_browser_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if cfg!(target_os = "linux") {
        paths.extend([
            "/snap/bin/chromium",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/opt/google/chrome/chrome",
            "/usr/bin/microsoft-edge",
            "/usr/bin/microsoft-edge-stable",
            "/opt/microsoft/msedge/msedge",
        ]);
    }

    if cfg!(target_os = "macos") {
        paths.extend([
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ]);
    }

    if cfg!(target_os = "windows") {
        paths.extend([
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        ]);
    }

    paths.into_iter().map(PathBuf::from).collect()
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
