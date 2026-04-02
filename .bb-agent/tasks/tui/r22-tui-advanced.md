# Task: implement advanced pi TUI features in BB

Worktree: `/tmp/bb-restructure/r22-tui-advanced`
Branch: `r22-tui-advanced`

## Goal
Port the remaining pi TUI features into BB's `crates/tui/src/tui_core.rs` and `crates/tui/src/renderer.rs`.

## Features to implement

### 1. Focused component tracking (tui_core.rs)
Pi tracks `focusedComponent` as a reference to the actual component, not an index. BB uses `focus_index`. Change to track by component reference:
- Add `focused_component: Option<*const dyn Component>` or keep index but add `set_focus_component(&mut self, component: &dyn Component)` 
- Actually, keep BB's index-based approach but add Focusable trait support:
  - When setting focus, call `old.set_focused(false)` and `new.set_focused(true)`
  - Pi does this in `setFocus()`

### 2. Overlay compositing with column positioning (tui_core.rs + renderer.rs)
Pi supports overlays at arbitrary (row, col) positions, not just bottom-anchored. Add:
- Full `OverlayOptions` struct with: width, minWidth, maxHeight, anchor, row, col, margin, offsetX, offsetY, nonCapturing, visible callback
- `resolve_overlay_layout()` method that computes position from options
- `composite_line_at()` that splices overlay content into base line at a specific column
- `composite_overlays()` that handles multiple positioned overlays

### 3. Segment reset / line reset (renderer.rs)  
Pi appends `\x1b[0m\x1b]8;;\x07` at end of each line (resets both SGR and hyperlink). Ensure BB's line_reset does the same:
- The `\x1b]8;;\x07` part resets hyperlink (OSC 8)
- Check if BB already does this or just `\x1b[0m`

### 4. Debug redraw logging (renderer.rs)
Pi logs full redraws to `~/.pi/agent/pi-debug.log` when `PI_DEBUG_REDRAW=1`. Add:
- Check `BB_DEBUG_REDRAW` env var
- Log reason for full redraws (width changed, height changed, clear_on_shrink, etc)

### 5. Termux session detection (tui_core.rs or renderer.rs)
Pi skips full redraw on height change when in Termux (keyboard show/hide changes height). Add:
- `fn is_termux() -> bool { std::env::var("TERMUX_VERSION").is_ok() }`
- Skip `height_changed` full redraw when in Termux

### 6. onDebug callback (tui_core.rs)
Pi has `Shift+Ctrl+D` triggers an `onDebug` callback. Add:
- `on_debug: Option<Box<dyn FnMut()>>` field
- In input handling, detect Shift+Ctrl+D and call it

### 7. Width overflow crash log (renderer.rs)
Pi writes a crash log when a line exceeds terminal width. Add:
- In render, if `visible_width(line) > width`, truncate and write debug info to `~/.bb-agent/tui-crash.log`
- Don't panic, just truncate and log

### 8. nonCapturing overlays (tui_core.rs)
Pi supports `nonCapturing: true` overlays that don't steal focus. Add:
- `non_capturing: bool` field in OverlayEntry
- Skip focus capture when showing a non-capturing overlay
- Skip non-capturing overlays in input dispatch

## Reference
Pi source: `/home/shuyhere/tmp/pi-mono/packages/tui/src/tui.ts` (1200 lines)
BB files: `crates/tui/src/tui_core.rs`, `crates/tui/src/renderer.rs`

## Constraints
- Don't break existing behavior
- Keep scrollback-based architecture
- Test with `cargo build -q` and `cargo test -q -p bb-tui`

## Finish
```
git add -A && git commit -m "port advanced pi TUI features: overlay positioning, debug, termux, focus tracking"
```
