# Task: r35 shared fullscreen controls

Worktree: `/tmp/bb-fullscreen-final/r35-controls`
Branch: `r35-shared-fullscreen-controls`

## Goal

Finish transcript interaction controls in the shared fullscreen path that now lives on `master` after commit `c360a71`.

## Current base state

`master` already has:
- fullscreen foundation
- shared structured transcript model
- shared fullscreen projection
- shared fullscreen viewport

What is still missing is the actual transcript interaction model in the shared fullscreen runtime.

## Implement in the shared fullscreen path only

Target files under:
- `crates/tui/src/fullscreen/`
- `crates/cli/src/interactive_fullscreen/`

Do NOT build another CLI-local fullscreen transcript subsystem.
Do NOT modify the old interactive controller path.

## Required features

### 1. UI modes
Add explicit fullscreen modes:
- normal
- transcript
- search (scaffold acceptable)

### 2. Keyboard controls
Implement in shared fullscreen runtime:
- `Ctrl+O` toggle transcript mode
- `j` / `k`
- Up / Down
- PgUp / PgDn
- Home / End
- `g` / `G`
- `Enter` / `Space` toggle focused block
- `o` expand focused block
- `c` collapse focused block
- `/` enter search mode
- `n` / `N` search traversal scaffold
- `Esc` exit search / transcript mode appropriately

### 3. Mouse controls
Implement in shared fullscreen runtime:
- wheel scrolls transcript viewport
- click on action/header row toggles expand/collapse

### 4. Focused block styling
A focused action/header row must be visually highlighted.
Keep it terminal-friendly and lightweight.

## Reuse / port from references only
References:
- rejected-but-informative branch `r27-input-modes-mouse`
- accepted shared fullscreen modules on `master`

Port behavior only, not the old architecture.

## Verification

```bash
cd /tmp/bb-fullscreen-final/r35-controls
cargo build
```

## Finish

```bash
git add -A && git commit -m "add transcript controls to shared fullscreen runtime"
```
