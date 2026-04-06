//! Centralized theme system for TUI colors.
//!
//! Replaces all hardcoded ANSI constants with a single [`Theme`] struct.
//! Respects `NO_COLOR` (disable all colors) and `FORCE_COLOR` (always emit colors).

use std::sync::LazyLock;

/// Returns the global theme instance.
pub fn theme() -> &'static Theme {
    &THEME
}

/// Global theme singleton. Respects `NO_COLOR` / `FORCE_COLOR` env vars.
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    if std::env::var("NO_COLOR").is_ok() {
        Theme::none()
    } else {
        Theme::dark()
    }
});

/// Convert a hex color like `#8abeb7` to an ANSI 24-bit foreground escape.
fn hex_fg(hex: &str) -> String {
    let (r, g, b) = parse_hex(hex);
    format!("\x1b[38;2;{r};{g};{b}m")
}

/// Convert a hex color like `#343541` to an ANSI 24-bit background escape.
fn hex_bg(hex: &str) -> String {
    let (r, g, b) = parse_hex(hex);
    format!("\x1b[48;2;{r};{g};{b}m")
}

fn parse_hex(hex: &str) -> (u8, u8, u8) {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    (r, g, b)
}

/// All themeable color slots used across the TUI.
#[derive(Debug, Clone)]
pub struct Theme {
    // Text styles
    pub text: String,
    pub dim: String,
    pub muted: String,
    pub bold: String,
    pub italic: String,

    // Semantic
    pub accent: String,
    pub success: String,
    pub error: String,
    pub warning: String,

    // Borders
    pub border: String,
    pub border_accent: String,
    pub border_muted: String,

    // User / selected / custom blocks
    pub user_msg_bg: String,
    pub selected_bg: String,
    pub custom_msg_bg: String,
    pub custom_msg_label: String,
    pub info_bg: String,

    // Tool execution
    pub tool_pending_bg: String,
    pub tool_success_bg: String,
    pub tool_error_bg: String,
    pub tool_title: String,
    pub tool_output: String,

    // Thinking
    pub thinking_text: String,

    // Markdown
    pub md_heading: String,
    pub md_link: String,
    pub md_code: String,
    pub md_code_block: String,

    // Diff
    pub diff_added: String,
    pub diff_removed: String,
    pub diff_context: String,
    pub diff_added_bg: String,
    pub diff_removed_bg: String,

    // Raw ANSI codes used in existing files
    pub green: String,
    pub red: String,
    pub yellow: String,
    pub cyan: String,

    // Reset
    pub reset: String,
}

impl Theme {
    /// Default dark theme for the fullscreen TUI.
    pub fn dark() -> Self {
        Self {
            // Text styles
            text: String::new(),
            dim: "\x1b[2m".into(),
            muted: hex_fg("#808080"),
            bold: "\x1b[1m".into(),
            italic: "\x1b[3m".into(),

            // Semantic colors
            accent: hex_fg("#8abeb7"),
            success: hex_fg("#b5bd68"),
            error: hex_fg("#cc6666"),
            warning: hex_fg("#ffff00"),

            // Borders
            border: hex_fg("#5f87ff"),
            border_accent: hex_fg("#00d7ff"),
            border_muted: hex_fg("#505050"),

            // User / selected / custom blocks
            user_msg_bg: hex_bg("#343541"),
            selected_bg: hex_bg("#3a3a4a"),
            custom_msg_bg: hex_bg("#2d2838"),
            custom_msg_label: hex_fg("#9575cd"),
            info_bg: hex_bg("#3c3728"),

            // Tool execution
            tool_pending_bg: hex_bg("#282832"),
            tool_success_bg: hex_bg("#283228"),
            tool_error_bg: hex_bg("#3c2828"),
            tool_title: String::new(),
            tool_output: hex_fg("#808080"),

            // Thinking
            thinking_text: hex_fg("#808080"),

            // Markdown
            md_heading: hex_fg("#f0c674"),
            md_link: hex_fg("#81a2be"),
            md_code: hex_fg("#8abeb7"),
            md_code_block: hex_fg("#b5bd68"),

            // Diff colors aligned with the rest of the tool palette
            diff_added: hex_fg("#b5bd68"),
            diff_removed: hex_fg("#cc6666"),
            diff_context: hex_fg("#808080"),
            diff_added_bg: hex_bg("#283228"),
            diff_removed_bg: hex_bg("#3c2828"),

            // Raw ANSI basic colors (kept for backward-compat with overlays)
            green: "\x1b[32m".into(),
            red: "\x1b[31m".into(),
            yellow: "\x1b[33m".into(),
            cyan: "\x1b[36m".into(),

            // Reset
            reset: "\x1b[0m".into(),
        }
    }

    /// A colorless theme — all fields are empty strings.
    /// Used when `NO_COLOR` env var is set.
    pub fn none() -> Self {
        Self {
            text: String::new(),
            dim: String::new(),
            muted: String::new(),
            bold: String::new(),
            italic: String::new(),
            accent: String::new(),
            success: String::new(),
            error: String::new(),
            warning: String::new(),
            border: String::new(),
            border_accent: String::new(),
            border_muted: String::new(),
            user_msg_bg: String::new(),
            selected_bg: String::new(),
            custom_msg_bg: String::new(),
            custom_msg_label: String::new(),
            info_bg: String::new(),
            tool_pending_bg: String::new(),
            tool_success_bg: String::new(),
            tool_error_bg: String::new(),
            tool_title: String::new(),
            tool_output: String::new(),
            thinking_text: String::new(),
            md_heading: String::new(),
            md_link: String::new(),
            md_code: String::new(),
            md_code_block: String::new(),
            diff_added: String::new(),
            diff_removed: String::new(),
            diff_context: String::new(),
            diff_added_bg: String::new(),
            diff_removed_bg: String::new(),
            green: String::new(),
            red: String::new(),
            yellow: String::new(),
            cyan: String::new(),
            reset: String::new(),
        }
    }

    /// Returns the foreground escape for a given hex color (runtime helper).
    pub fn fg(hex: &str) -> String {
        hex_fg(hex)
    }

    /// Returns the background escape for a given hex color (runtime helper).
    pub fn bg(hex: &str) -> String {
        hex_bg(hex)
    }

    /// Whether colors are enabled (i.e. not the `none` theme).
    pub fn colors_enabled(&self) -> bool {
        !self.reset.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_theme_has_reset() {
        let t = Theme::dark();
        assert_eq!(t.reset, "\x1b[0m");
        assert!(t.colors_enabled());
    }

    #[test]
    fn none_theme_is_empty() {
        let t = Theme::none();
        assert!(t.reset.is_empty());
        assert!(t.accent.is_empty());
        assert!(!t.colors_enabled());
    }

    #[test]
    fn hex_fg_produces_ansi() {
        assert_eq!(hex_fg("#ff0000"), "\x1b[38;2;255;0;0m");
        assert_eq!(hex_fg("#808080"), "\x1b[38;2;128;128;128m");
    }

    #[test]
    fn hex_bg_produces_ansi() {
        assert_eq!(hex_bg("#343541"), "\x1b[48;2;52;53;65m");
    }

    #[test]
    fn global_theme_accessible() {
        let t = theme();
        // Should be one of the two variants
        assert!(t.reset == "\x1b[0m" || t.reset.is_empty());
    }
}
