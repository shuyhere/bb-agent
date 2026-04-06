use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{LazyLock, Mutex};

use regex::Regex;

static GITHUB_REF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)#(\d+)\b").expect("valid github ref regex")
});
static ISSUE_ONLY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[^A-Za-z0-9_/.-])#(\d+)\b").expect("valid issue-only regex"));
static GITHUB_REMOTE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"github\.com[:/]([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+?)(?:\.git)?$")
        .expect("valid github remote regex")
});
static GITHUB_REPO_CACHE: LazyLock<Mutex<HashMap<PathBuf, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn create_hyperlink(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x07{text}\x1b]8;;\x07")
}

pub(super) fn auto_link_github_refs(text: &str) -> String {
    auto_link_github_refs_with_repo(text, current_github_repo_slug().as_deref())
}

pub(super) fn auto_link_github_refs_with_repo(text: &str, current_repo: Option<&str>) -> String {
    let mut out = String::new();
    let mut last = 0usize;
    for captures in GITHUB_REF_RE.captures_iter(text) {
        let Some(matched) = captures.get(0) else {
            continue;
        };
        out.push_str(&text[last..matched.start()]);
        let owner = captures
            .get(1)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let repo = captures
            .get(2)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let number = captures
            .get(3)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let url = format!("https://github.com/{owner}/{repo}/issues/{number}");
        out.push_str(&create_hyperlink(&url, matched.as_str()));
        last = matched.end();
    }
    out.push_str(&text[last..]);

    let Some(current_repo) = current_repo else {
        return out;
    };

    let mut second_pass = String::new();
    let mut last = 0usize;
    for captures in ISSUE_ONLY_RE.captures_iter(&out) {
        let Some(matched) = captures.get(0) else {
            continue;
        };
        let Some(prefix) = captures.get(1) else {
            continue;
        };
        let Some(number) = captures.get(2) else {
            continue;
        };
        let issue_start = matched.end().saturating_sub(number.as_str().len() + 1);
        second_pass.push_str(&out[last..prefix.end()]);
        let issue_text = &out[issue_start..matched.end()];
        let url = format!(
            "https://github.com/{current_repo}/issues/{}",
            number.as_str()
        );
        second_pass.push_str(&create_hyperlink(&url, issue_text));
        last = matched.end();
    }
    second_pass.push_str(&out[last..]);
    second_pass
}

fn current_github_repo_slug() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    if let Ok(cache) = GITHUB_REPO_CACHE.lock()
        && let Some(cached) = cache.get(&cwd)
    {
        return cached.clone();
    }

    let remote = Command::new("git")
        .arg("config")
        .arg("--get")
        .arg("remote.origin.url")
        .current_dir(&cwd)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|remote| !remote.is_empty());

    let slug = remote.and_then(|remote| {
        let captures = GITHUB_REMOTE_RE.captures(&remote)?;
        let owner = captures.get(1)?.as_str();
        let repo = captures.get(2)?.as_str();
        Some(format!("{owner}/{repo}"))
    });

    if let Ok(mut cache) = GITHUB_REPO_CACHE.lock() {
        cache.insert(cwd, slug.clone());
    }
    slug
}
