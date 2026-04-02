# Task: implement theme engine for BB-Agent TUI

Worktree: `/tmp/bb-restructure/r23-theme`
Branch: `r23-theme-engine`

## Goal
Replace all hardcoded ANSI color constants across the TUI with a centralized Theme system, matching pi's theme architecture.

## What to implement

### 1. Create `crates/tui/src/theme.rs`

```rust
pub struct Theme {
    // Text
    pub text: String,        // default text (empty = terminal default)
    pub dim: String,         // dim/muted text (#666666)
    pub muted: String,       // gray text (#808080)
    pub bold: String,        // bold marker

    // Semantic
    pub accent: String,      // accent color (#8abeb7)
    pub success: String,     // green (#b5bd68)
    pub error: String,       // red (#cc6666)
    pub warning: String,     // yellow (#ffff00)

    // Borders
    pub border: String,      // border color (#5f87ff)
    pub border_accent: String, // accent border (#00d7ff)
    pub border_muted: String,  // dark border (#505050)

    // User message
    pub user_msg_bg: String, // (#343541)

    // Tool execution
    pub tool_pending_bg: String,  // (#282832)
    pub tool_success_bg: String,  // (#283228)
    pub tool_error_bg: String,    // (#3c2828)
    pub tool_title: String,       // default
    pub tool_output: String,      // gray (#808080)

    // Thinking
    pub thinking_text: String, // gray (#808080)

    // Markdown
    pub md_heading: String,   // (#f0c674)
    pub md_link: String,      // (#81a2be)
    pub md_code: String,      // accent
    pub md_code_block: String, // green

    // Diff
    pub diff_added: String,   // green
    pub diff_removed: String, // red
    pub diff_context: String, // gray

    // Reset
    pub reset: String,        // \x1b[0m
}
```

Add `Theme::dark() -> Self` that returns the default dark theme matching pi's dark.json values.

Add `pub static THEME: once_cell::sync::Lazy<Theme>` or a thread-local for global access.

Add helper methods:
- `theme.fg(color_field) -> &str` — returns the ANSI escape for a named color
- `theme.bg(color_field) -> &str` — returns background version

### 2. Replace hardcoded colors

Replace all hardcoded ANSI constants in these files:

**`crates/cli/src/interactive/components/tool_execution.rs`:**
- `RESET`, `DIM`, `BOLD`, `ACCENT`, `SUCCESS`, `ERROR`, `MUTED`
- `TOOL_OUTPUT`, `TOOL_PENDING_BG`, `TOOL_SUCCESS_BG`, `TOOL_ERROR_BG`

**`crates/cli/src/interactive/components/assistant_message.rs`:**
- `RESET`, `ITALIC`, `THINKING_COLOR`, `ERROR_COLOR`

**`crates/tui/src/footer.rs`:**
- `DIM`, `RESET`, `RED`, `YELLOW`

**`crates/cli/src/interactive/controller/rendering.rs`:**
- user_bg color, dim colors

**`crates/cli/src/interactive/auth_selector_overlay.rs`:**
- border_color, green, dim, bold

**`crates/cli/src/interactive/session_selector_overlay.rs`:**
- border_color, green, dim, bold

**`crates/cli/src/interactive/tree_selector_overlay.rs`:**
- border_color, green, cyan, yellow, dim, bold

**`crates/cli/src/interactive/settings_overlay.rs`:**
- border_color, accent, dim, bold

Each file should `use bb_tui::theme::theme;` and reference `theme().accent` etc instead of hardcoded strings.

### 3. FORCE_COLOR / NO_COLOR support

In `theme.rs`, check:
- `NO_COLOR` env var → return a theme with all color fields empty (no ANSI codes)
- `FORCE_COLOR` env var → always use colors even if not a TTY

### 4. Register in lib.rs

Add `pub mod theme;` to `crates/tui/src/lib.rs`.

## Pi reference
Pi's theme: `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/modes/interactive/theme/dark.json`

Key values:
```json
{
  "accent": "#8abeb7",
  "border": "#5f87ff",
  "success": "#b5bd68",
  "error": "#cc6666",
  "warning": "#ffff00",
  "muted": "#808080",
  "dim": "#666666",
  "toolPendingBg": "#282832",
  "toolSuccessBg": "#283228",
  "toolErrorBg": "#3c2828",
  "userMsgBg": "#343541",
  "toolOutput": "#808080",
  "thinkingText": "#808080"
}
```

## Constraints
- Don't change visual appearance (same colors, just centralized)
- Keep backward compatibility
- All files must compile
- Use `once_cell` or `std::sync::LazyLock` for global theme

## Verification
```
cargo build -q
cargo test -q -p bb-tui
```

## Finish
```
git add -A && git commit -m "implement theme engine: centralize all colors, support NO_COLOR/FORCE_COLOR"
```
