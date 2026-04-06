use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, PartialEq, Eq)]
struct GitPaths {
    repo_dir: PathBuf,
    common_git_dir: PathBuf,
    head_path: PathBuf,
}

fn find_git_paths(cwd: &Path) -> Option<GitPaths> {
    let mut dir = cwd.to_path_buf();

    loop {
        let git_path = dir.join(".git");
        if git_path.exists() {
            let stat = fs::metadata(&git_path).ok()?;
            if stat.is_file() {
                let content = fs::read_to_string(&git_path).ok()?;
                let content = content.trim();
                if let Some(rest) = content.strip_prefix("gitdir: ") {
                    let git_dir = dir.join(rest.trim()).canonicalize().ok()?;
                    let head_path = git_dir.join("HEAD");
                    if !head_path.exists() {
                        return None;
                    }
                    let common_dir_path = git_dir.join("commondir");
                    let common_git_dir = if common_dir_path.exists() {
                        let common_dir = fs::read_to_string(common_dir_path).ok()?;
                        git_dir.join(common_dir.trim()).canonicalize().ok()?
                    } else {
                        git_dir.clone()
                    };
                    return Some(GitPaths {
                        repo_dir: dir,
                        common_git_dir,
                        head_path,
                    });
                }
            } else if stat.is_dir() {
                let head_path = git_path.join("HEAD");
                if !head_path.exists() {
                    return None;
                }
                return Some(GitPaths {
                    repo_dir: dir,
                    common_git_dir: git_path,
                    head_path,
                });
            }
        }

        if !dir.pop() {
            return None;
        }
    }
}

fn resolve_branch_with_git_sync(repo_dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args([
            "--no-optional-locks",
            "symbolic-ref",
            "--quiet",
            "--short",
            "HEAD",
        ])
        .current_dir(repo_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

#[derive(Clone, Debug)]
pub struct FooterDataProvider {
    cwd: PathBuf,
    extension_statuses: BTreeMap<String, String>,
    cached_branch: RefCell<Option<Option<String>>>,
    git_paths: Option<GitPaths>,
    available_provider_count: usize,
}

impl FooterDataProvider {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let git_paths = find_git_paths(&cwd);
        Self {
            cwd,
            extension_statuses: BTreeMap::new(),
            cached_branch: RefCell::new(None),
            git_paths,
            available_provider_count: 0,
        }
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn get_git_branch(&self) -> Option<String> {
        if self.cached_branch.borrow().is_none() {
            *self.cached_branch.borrow_mut() = Some(self.resolve_git_branch_sync());
        }
        self.cached_branch.borrow().clone().flatten()
    }

    pub fn get_extension_statuses(&self) -> &BTreeMap<String, String> {
        &self.extension_statuses
    }

    pub fn set_extension_status(&mut self, key: impl Into<String>, text: Option<String>) {
        let key = key.into();
        if let Some(text) = text {
            self.extension_statuses.insert(key, text);
        } else {
            self.extension_statuses.remove(&key);
        }
    }

    pub fn clear_extension_statuses(&mut self) {
        self.extension_statuses.clear();
    }

    pub fn get_available_provider_count(&self) -> usize {
        self.available_provider_count
    }

    pub fn set_available_provider_count(&mut self, count: usize) {
        self.available_provider_count = count;
    }

    pub fn set_cwd(&mut self, cwd: impl Into<PathBuf>) {
        let cwd = cwd.into();
        if self.cwd == cwd {
            return;
        }

        self.cwd = cwd;
        *self.cached_branch.borrow_mut() = None;
        self.git_paths = find_git_paths(&self.cwd);
    }

    pub fn invalidate_git_branch(&self) {
        *self.cached_branch.borrow_mut() = None;
    }

    fn resolve_git_branch_sync(&self) -> Option<String> {
        let git_paths = self.git_paths.as_ref()?;
        let content = fs::read_to_string(&git_paths.head_path).ok()?;
        let content = content.trim();
        if let Some(branch) = content.strip_prefix("ref: refs/heads/") {
            if branch == ".invalid" {
                return Some(
                    resolve_branch_with_git_sync(&git_paths.repo_dir)
                        .unwrap_or_else(|| "detached".to_string()),
                );
            }
            return Some(branch.to_string());
        }
        Some("detached".to_string())
    }
}

impl Default for FooterDataProvider {
    fn default() -> Self {
        Self::new(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

pub type ReadonlyFooterDataProvider = FooterDataProvider;
