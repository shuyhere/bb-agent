# Task: r24 fullscreen foundation

Worktree: `/tmp/bb-fullscreen/r24-foundation`
Branch: `r24-fullscreen-foundation`

## Goal

Create the first fullscreen transcript shell for BB-Agent.
This is the terminal ownership and layout branch.

## User-approved direction

This task intentionally shifts BB-Agent away from the current scrollback-style interactive renderer for the new path.
Implement a new fullscreen transcript mode that uses:

- alternate screen buffer
- raw mode
- mouse capture
- bottom-fixed input area
- dedicated transcript viewport
- dedicated status line
- stable no-flicker frame rendering

Do not delete the old interactive path yet.
Build the new path in parallel.

## Main deliverables

### 1. Add a new fullscreen path
Create new modules for a fullscreen transcript UI rather than mutating the current interactive controller in place.
Suggested locations:

- `crates/tui/src/fullscreen/`
- `crates/cli/src/interactive_fullscreen/`

Follow existing Rust design rules:
- one file, one responsibility
- `mod.rs` as router only
- explicit state / orchestration / implementation split

### 2. Terminal ownership
Add terminal setup and teardown for:

- EnterAlternateScreen
- LeaveAlternateScreen
- EnableMouseCapture
- DisableMouseCapture
- raw mode lifecycle
- cursor hide/show lifecycle
- synchronized update if supported

The fullscreen path should fully own the terminal while active.

### 3. Layout shell
Implement a stable layout like:

- transcript viewport
- input box
- status line

The input area must stay fixed at the bottom.
The transcript viewport must consume the remaining height.

### 4. Render loop skeleton
Implement an event loop skeleton that supports:

- keyboard input
- mouse input
- resize input
- timer tick
- state dirty flag
- render scheduling

### 5. Optional dependency choice
The user-provided plan recommends `crossterm + ratatui`.
If you adopt ratatui for the new fullscreen path, keep it isolated to the new modules and do not break the existing build.

## Constraints

- Preserve current BB build and current interactive mode.
- The new fullscreen mode should be introduced behind a dedicated switch, env var, or internal constructor.
- Do not remove existing functionality yet.
- Avoid full-screen clears during routine updates.

## Suggested files to inspect

- `crates/tui/`
- `crates/cli/src/interactive.rs`
- `crates/cli/src/run.rs`
- `crates/cli/src/main.rs`

## Verification

```bash
cd /tmp/bb-fullscreen/r24-foundation
cargo build
```

## Finish

```bash
git add -A && git commit -m "add fullscreen transcript foundation"
```
