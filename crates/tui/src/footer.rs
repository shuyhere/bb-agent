//! Footer component — ported from pi's interactive footer.
//!
//! Shows:
//!   Line 1: cwd (branch) • session-name
//!   Line 2: ↑input ↓output $cost  context%/window (auto)    (provider) model • thinking

use std::process::{Command, Stdio};

use crate::component::Component;
use crate::utils::{truncate_to_width, visible_width};

pub use crate::footer_data::{FooterDataProvider, ReadonlyFooterDataProvider};

/// Detect git branch for the given cwd.
pub fn detect_git_branch(cwd: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["--no-optional-locks", "symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() {
            Some("detached".to_string())
        } else {
            Some(branch)
        }
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
    /// Whether the current model is using an OAuth subscription (shows "(sub)" after cost).
    pub is_subscription: bool,
}

impl Default for FooterData {
    fn default() -> Self {
        Self {
            model_name: "no-model".into(),
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
            is_subscription: false,
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
    if count < 1_000 {
        count.to_string()
    } else if count < 10_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else if count < 1_000_000 {
        format!("{}k", (count as f64 / 1_000.0).round() as u64)
    } else if count < 10_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else {
        format!("{}M", (count as f64 / 1_000_000.0).round() as u64)
    }
}

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";

impl Component for Footer {
    crate::impl_as_any!();

    fn render(&self, width: u16) -> Vec<String> {
        let width = width as usize;
        let data = &self.data;

        let mut pwd = data.cwd.clone();
        if let Ok(home) = std::env::var("HOME") {
            if pwd.starts_with(&home) {
                pwd = format!("~{}", &pwd[home.len()..]);
            }
        }
        if let Some(branch) = &data.git_branch {
            pwd = format!("{pwd} ({branch})");
        }
        if let Some(session_name) = &data.session_name {
            pwd = format!("{pwd} • {session_name}");
        }

        let pwd_line = truncate_to_width(&format!("{DIM}{pwd}{RESET}"), width);

        let mut stats_parts = Vec::new();
        if data.input_tokens > 0 {
            stats_parts.push(format!("↑{}", format_tokens(data.input_tokens)));
        }
        if data.output_tokens > 0 {
            stats_parts.push(format!("↓{}", format_tokens(data.output_tokens)));
        }
        if data.cache_read > 0 {
            stats_parts.push(format!("R{}", format_tokens(data.cache_read)));
        }
        if data.cache_write > 0 {
            stats_parts.push(format!("W{}", format_tokens(data.cache_write)));
        }
        if data.cost > 0.0 || data.is_subscription {
            let sub = if data.is_subscription { " (sub)" } else { "" };
            stats_parts.push(format!("${:.3}{sub}", data.cost));
        }

        let context_percent_value = data.context_percent.unwrap_or(0.0);
        let auto_indicator = if data.auto_compact { " (auto)" } else { "" };
        let context_display = match data.context_percent {
            Some(percent) => format!(
                "{percent:.1}%/{}{}",
                format_tokens(data.context_window),
                auto_indicator
            ),
            None => format!("?/{}{}", format_tokens(data.context_window), auto_indicator),
        };
        let context_percent_str = if context_percent_value > 90.0 {
            format!("{RED}{context_display}{RESET}")
        } else if context_percent_value > 70.0 {
            format!("{YELLOW}{context_display}{RESET}")
        } else {
            context_display
        };
        stats_parts.push(context_percent_str);

        let mut stats_left = stats_parts.join(" ");
        let mut stats_left_width = visible_width(&stats_left);
        if stats_left_width > width {
            stats_left = truncate_to_width(&stats_left, width);
            stats_left_width = visible_width(&stats_left);
        }

        let mut right_side_without_provider = data.model_name.clone();
        if let Some(thinking_level) = &data.thinking_level {
            right_side_without_provider = if thinking_level == "off" {
                format!("{} • thinking off", data.model_name)
            } else {
                format!("{} • {}", data.model_name, thinking_level)
            };
        }

        let min_padding = 2;
        let mut right_side = right_side_without_provider.clone();
        if data.available_provider_count > 1 && !data.provider.is_empty() {
            let with_provider = format!("({}) {}", data.provider, right_side_without_provider);
            if stats_left_width + min_padding + visible_width(&with_provider) <= width {
                right_side = with_provider;
            }
        }

        let right_side_width = visible_width(&right_side);
        let total_needed = stats_left_width + min_padding + right_side_width;

        let stats_line = if total_needed <= width {
            let padding = " ".repeat(width.saturating_sub(stats_left_width + right_side_width));
            format!("{stats_left}{padding}{right_side}")
        } else {
            let available_for_right = width.saturating_sub(stats_left_width + min_padding);
            if available_for_right > 0 {
                let truncated_right = truncate_to_width(&right_side, available_for_right);
                let truncated_right_width = visible_width(&truncated_right);
                let padding = " ".repeat(width.saturating_sub(stats_left_width + truncated_right_width));
                format!("{stats_left}{padding}{truncated_right}")
            } else {
                stats_left.clone()
            }
        };

        let remainder = &stats_line[stats_left.len()..];
        let dim_stats_left = format!("{DIM}{stats_left}{RESET}");
        let dim_remainder = format!("{DIM}{remainder}{RESET}");

        vec![pwd_line, format!("{dim_stats_left}{dim_remainder}")]
    }

    fn invalidate(&mut self) {}
}
