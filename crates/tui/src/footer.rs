//! Footer component — matches pi's footer layout.
//!
//! Shows:
//!   Line 1: cwd (branch) • session-name
//!   Line 2: ↑input ↓output $cost  context%/window (auto)    (provider) model • thinking

use crate::component::Component;
use crate::utils::{truncate_to_width, visible_width};

/// Detect git branch for the given cwd.
pub fn detect_git_branch(cwd: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() { None } else { Some(branch) }
    } else {
        None
    }
}

/// Data needed to render the footer.
#[derive(Clone, Debug)]
pub struct FooterData {
    /// Model identifier shown in footer (pi uses model id, not display name)
    pub model_name: String,
    pub provider: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub session_name: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: f64,
    pub context_percent: Option<f64>,
    pub context_window: u64,
    pub auto_compact: bool,
    pub thinking_level: Option<String>,
    pub available_provider_count: usize,
}

impl Default for FooterData {
    fn default() -> Self {
        Self {
            model_name: "unknown".into(),
            provider: "unknown".into(),
            cwd: ".".into(),
            git_branch: None,
            session_name: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read: 0,
            cache_write: 0,
            cost: 0.0,
            context_percent: None,
            context_window: 128_000,
            auto_compact: true,
            thinking_level: None,
            available_provider_count: 1,
        }
    }
}

/// Footer component that shows status information.
pub struct Footer {
    pub data: FooterData,
}

impl Footer {
    pub fn new(data: FooterData) -> Self {
        Self { data }
    }
}

fn format_tokens(count: u64) -> String {
    if count < 1000 {
        count.to_string()
    } else if count < 10_000 {
        format!("{:.1}k", count as f64 / 1000.0)
    } else if count < 1_000_000 {
        format!("{}k", count / 1000)
    } else if count < 10_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else {
        format!("{}M", count / 1_000_000)
    }
}

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";

impl Component for Footer {
    crate::impl_as_any!();

    fn render(&self, width: u16) -> Vec<String> {
        let w = width as usize;
        let d = &self.data;

        // ── Line 1: cwd (branch) • session ──
        let mut pwd = d.cwd.clone();
        if let Ok(home) = std::env::var("HOME") {
            if pwd.starts_with(&home) {
                pwd = format!("~{}", &pwd[home.len()..]);
            }
        }
        if let Some(ref branch) = d.git_branch {
            pwd = format!("{} ({})", pwd, branch);
        }
        if let Some(ref name) = d.session_name {
            pwd = format!("{} • {}", pwd, name);
        }
        let pwd_line = truncate_to_width(&format!("{DIM}{pwd}{RESET}"), w);

        // ── Line 2: stats left ... model right ──
        let mut parts = Vec::new();
        if d.input_tokens > 0 {
            parts.push(format!("↑{}", format_tokens(d.input_tokens)));
        }
        if d.output_tokens > 0 {
            parts.push(format!("↓{}", format_tokens(d.output_tokens)));
        }
        if d.cache_read > 0 {
            parts.push(format!("R{}", format_tokens(d.cache_read)));
        }
        if d.cache_write > 0 {
            parts.push(format!("W{}", format_tokens(d.cache_write)));
        }
        if d.cost > 0.0 {
            parts.push(format!("${:.3}", d.cost));
        }

        // Context percentage
        let auto_indicator = if d.auto_compact { " (auto)" } else { "" };
        let context_display = match d.context_percent {
            Some(pct) => format!(
                "{:.1}%/{}{}",
                pct,
                format_tokens(d.context_window),
                auto_indicator,
            ),
            None => format!(
                "?/{}{}",
                format_tokens(d.context_window),
                auto_indicator,
            ),
        };

        // Colorize context by usage
        let pct_val = d.context_percent.unwrap_or(0.0);
        let context_str = if pct_val > 90.0 {
            format!("{RED}{context_display}{RESET}")
        } else if pct_val > 70.0 {
            format!("{YELLOW}{context_display}{RESET}")
        } else {
            context_display
        };
        parts.push(context_str);

        let stats_left = parts.join(" ");
        let stats_left_width = visible_width(&stats_left);

        // Right side: pi uses model id, plus thinking status when reasoning is supported
        let mut right = d.model_name.clone();
        if let Some(ref level) = d.thinking_level {
            right = if level == "off" {
                format!("{} • thinking off", right)
            } else {
                format!("{} • {}", right, level)
            };
        }
        // Match pi: only prepend provider when multiple providers are available.
        if d.available_provider_count > 1 {
            let right_with_provider = format!("({}) {}", d.provider, right);
            let right_with_provider_width = visible_width(&right_with_provider);
            if stats_left_width + 2 + right_with_provider_width <= w {
                right = right_with_provider;
            }
        }
        let right_width = visible_width(&right);

        let min_padding = 2;
        let total_needed = stats_left_width + min_padding + right_width;

        let stats_line = if total_needed <= w {
            let padding = " ".repeat(w - stats_left_width - right_width);
            format!("{stats_left}{padding}{right}")
        } else {
            // Truncate right
            let avail = w.saturating_sub(stats_left_width + min_padding);
            if avail > 0 {
                let truncated = truncate_to_width(&right, avail);
                let tw = visible_width(&truncated);
                let padding = " ".repeat(w.saturating_sub(stats_left_width + tw));
                format!("{stats_left}{padding}{truncated}")
            } else {
                stats_left
            }
        };

        let dim_stats = format!("{DIM}{stats_line}{RESET}");

        vec![pwd_line, dim_stats]
    }

    fn invalidate(&mut self) {}
}
