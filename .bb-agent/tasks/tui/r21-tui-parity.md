# Task: implement missing pi TUI features in BB

Worktree: `/tmp/bb-restructure/r21-tui-parity`
Branch: `r21-tui-parity`

## Goal
Port the remaining pi TUI features that BB is missing. These are in `crates/tui/src/tui_core.rs` and `crates/tui/src/renderer.rs`.

## Features to implement

### 1. `request_render()` batching (tui_core.rs)
Pi's `requestRender()` defers rendering to nextTick to batch multiple render requests. In BB, add a `render_requested` flag and a `flush_render()` method. Not critical for Rust (no event loop like Node) but add the flag to prevent redundant renders within a single event handler.

### 2. `clear_on_shrink` setting (renderer.rs)
When content shrinks (fewer lines than before), clear the empty rows. Pi has `setClearOnShrink(enabled)`. Add:
- `clear_on_shrink: bool` field to Renderer (default false)
- In `render()`, if `new_lines.len() < self.max_lines_rendered && !has_overlay`, do a full clear+render

### 3. Hardware cursor positioning (renderer.rs)
Pi positions the hardware cursor at the CURSOR_MARKER location for IME support. BB already extracts CURSOR_MARKER but may not position the terminal cursor there. Add:
- `position_hardware_cursor()` method that moves cursor to the marker position
- `show_hardware_cursor: bool` setting (default false)
- When enabled, show the terminal cursor and position it at the marker

### 4. Line reset / segment reset (renderer.rs)
Pi has LINE_RESET_MARKER (`\x1b]133;A\x07`) and SEGMENT_RESET. These reset ANSI state at line boundaries to prevent color bleed. Add:
- `apply_line_resets()` that appends `\x1b[0m` at the end of each line
- This prevents ANSI escape codes from one line bleeding into the next

### 5. Input middleware (tui_core.rs)
Pi has `addInputListener()` that lets components intercept/transform input before it reaches the focused component. Add:
- `input_listeners: Vec<Box<dyn Fn(&str) -> InputResult>>` 
- In `handle_key()` and `handle_raw_input()`, run through listeners first

### 6. Width overflow protection (renderer.rs)
Pi crashes (with debug log) if a rendered line exceeds terminal width. Add a check in `render()`:
- If `visible_width(line) > width`, truncate the line and log a warning (don't crash in BB)

### 7. Overlay positioning options (tui_core.rs)  
Pi supports overlay positioning: anchor (center/bottom), margins, maxHeight. BB only has bottom-anchored. Add:
- `OverlayOptions` struct with `anchor: OverlayAnchor` enum (Bottom, Center)
- For center: place overlay in the middle of the terminal
- For bottom (default): current behavior

## Files to modify
- `crates/tui/src/tui_core.rs`
- `crates/tui/src/renderer.rs`

## Constraints
- Don't break existing overlay/render behavior
- Keep the scrollback-based architecture
- Test with `cargo build -q` and `cargo test -q -p bb-tui`

## Finish
```
git add -A && git commit -m "port remaining pi TUI features: cursor, clear-on-shrink, line-reset, input-middleware"
```
