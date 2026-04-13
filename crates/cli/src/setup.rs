use anyhow::Result;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellKind {
    Bash,
    Zsh,
    Fish,
    Sh,
}

pub fn handle_setup_browser(persist: bool) -> Result<()> {
    match bb_tools::browser_fetch::resolve_browser_executable_path() {
        Some(browser) => report_found_browser(&browser, persist),
        None => {
            println!("No browser_fetch-compatible browser was detected.\n");
            println!("{}", bb_tools::browser_fetch::missing_browser_setup_message());
            println!();
            print_browser_install_guidance();
            println!();
            println!("After installing a browser, rerun:");
            println!("  bb setup browser --persist");
            Ok(())
        }
    }
}

fn report_found_browser(browser: &Path, persist: bool) -> Result<()> {
    let shell_kind = detect_shell_kind(env::var("SHELL").ok().as_deref());
    let export_line = browser_export_line(shell_kind, browser);

    println!("Found browser_fetch-compatible browser:\n  {}", browser.display());
    println!();
    println!("Use it in the current shell:");
    println!("  {export_line}");

    if persist {
        let rc_path = detect_shell_rc_path(shell_kind)?;
        let changed = persist_browser_export(&rc_path, shell_kind, browser)?;
        println!();
        if changed {
            println!("Saved BB_BROWSER to {}", rc_path.display());
        } else {
            println!("BB_BROWSER was already configured in {}", rc_path.display());
        }
        println!("Open a new shell, or run:");
        println!("  source {}", rc_path.display());
    } else if let Ok(rc_path) = detect_shell_rc_path(shell_kind) {
        println!();
        println!("To persist it for future shells, run:");
        println!("  bb setup browser --persist");
        println!("  # writes BB_BROWSER to {}", rc_path.display());
    }

    Ok(())
}

fn print_browser_install_guidance() {
    if cfg!(target_os = "linux") {
        println!("Linux install hints:");
        println!("  Ubuntu: sudo snap install chromium");
        println!("  Then rerun: bb setup browser --persist");
    } else if cfg!(target_os = "macos") {
        println!("macOS install hints:");
        println!("  Install Google Chrome or Chromium, then rerun: bb setup browser --persist");
    } else if cfg!(target_os = "windows") {
        println!("Windows install hints:");
        println!("  Install Google Chrome / Microsoft Edge, then rerun: bb setup browser --persist");
    } else {
        println!("Install a Chrome/Chromium-compatible browser, then rerun: bb setup browser --persist");
    }
}

fn detect_shell_kind(shell: Option<&str>) -> ShellKind {
    match shell.unwrap_or_default() {
        value if value.contains("fish") => ShellKind::Fish,
        value if value.contains("zsh") => ShellKind::Zsh,
        value if value.contains("bash") => ShellKind::Bash,
        _ => ShellKind::Sh,
    }
}

fn detect_shell_rc_path(shell_kind: ShellKind) -> Result<PathBuf> {
    let home = env::var("HOME")?;
    let home = PathBuf::from(home);
    let path = match shell_kind {
        ShellKind::Fish => home.join(".config/fish/config.fish"),
        ShellKind::Zsh => home.join(".zshrc"),
        ShellKind::Bash => home.join(".bashrc"),
        ShellKind::Sh => home.join(".profile"),
    };
    Ok(path)
}

fn browser_export_line(shell_kind: ShellKind, browser: &Path) -> String {
    let escaped = escape_double_quoted_path(browser);
    match shell_kind {
        ShellKind::Fish => format!("set -gx BB_BROWSER \"{escaped}\""),
        _ => format!("export BB_BROWSER=\"{escaped}\""),
    }
}

fn escape_double_quoted_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn persist_browser_export(rc_path: &Path, shell_kind: ShellKind, browser: &Path) -> Result<bool> {
    let export_line = browser_export_line(shell_kind, browser);
    let existing = fs::read_to_string(rc_path).unwrap_or_default();
    let updated = upsert_browser_export(&existing, shell_kind, &export_line);
    if updated == existing {
        return Ok(false);
    }

    if let Some(parent) = rc_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(rc_path, updated)?;
    Ok(true)
}

fn upsert_browser_export(existing: &str, shell_kind: ShellKind, export_line: &str) -> String {
    let mut replaced = false;
    let mut lines = Vec::new();

    for line in existing.lines() {
        let trimmed = line.trim_start();
        let is_browser_line = match shell_kind {
            ShellKind::Fish => {
                trimmed.starts_with("set -gx BB_BROWSER ")
                    || trimmed.starts_with("set -x BB_BROWSER ")
            }
            _ => trimmed.starts_with("export BB_BROWSER="),
        };

        if is_browser_line {
            if !replaced {
                lines.push(export_line.to_string());
                replaced = true;
            }
            continue;
        }

        lines.push(line.to_string());
    }

    if !replaced {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.is_empty()) {
            lines.push(String::new());
        }
        lines.push("# Added by bb setup browser".to_string());
        lines.push(export_line.to_string());
    }

    let mut output = lines.join("\n");
    if !output.is_empty() {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_shell_kind_from_shell_path() {
        assert_eq!(detect_shell_kind(Some("/bin/zsh")), ShellKind::Zsh);
        assert_eq!(detect_shell_kind(Some("/usr/bin/fish")), ShellKind::Fish);
        assert_eq!(detect_shell_kind(Some("/bin/bash")), ShellKind::Bash);
        assert_eq!(detect_shell_kind(Some("/bin/sh")), ShellKind::Sh);
    }

    #[test]
    fn sh_export_is_inserted_with_marker() {
        let updated = upsert_browser_export("alias ll='ls -l'\n", ShellKind::Bash, "export BB_BROWSER=\"/snap/bin/chromium\"");
        assert!(updated.contains("# Added by bb setup browser"));
        assert!(updated.contains("export BB_BROWSER=\"/snap/bin/chromium\""));
    }

    #[test]
    fn existing_export_is_replaced_once() {
        let updated = upsert_browser_export(
            "export BB_BROWSER=\"/old/path\"\nexport PATH=\"/tmp:$PATH\"\n",
            ShellKind::Zsh,
            "export BB_BROWSER=\"/new/path\"",
        );
        assert!(updated.contains("export BB_BROWSER=\"/new/path\""));
        assert!(!updated.contains("/old/path"));
    }

    #[test]
    fn fish_export_uses_set_gx() {
        let line = browser_export_line(
            ShellKind::Fish,
            Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        );
        assert_eq!(
            line,
            "set -gx BB_BROWSER \"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome\""
        );
    }
}
