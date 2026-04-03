# Task: r40 shared fullscreen streaming final

Worktree: `/tmp/bb-fullscreen-finish/r40-streaming`
Branch: `r40-shared-fullscreen-streaming-final`

## Goal

Finish the shared fullscreen streaming scheduler on the correct architecture.

## Starting point

Base from current `master`, then first integrate the accepted fullscreen architecture before salvaging streaming ideas.

## Mandatory first step

Bring the branch onto the accepted fullscreen base:
1. cherry-pick `c990227`
2. cherry-pick `e5796f5`

Resolve conflicts so the branch uses:
- `crates/tui/src/fullscreen/*`
- `crates/cli/src/fullscreen_entry.rs`

## Salvage source

Salvage ideas from:
- `fcb193c` from `r36-shared-fullscreen-streaming`

Port behavior only where needed. Do not preserve obsolete CLI-local fullscreen files.

## Required deliverables

### 1. Shared scheduler
Implement or finish in the shared fullscreen stack:
- dirty flag / dirty block tracking
- frame cadence cap
- idle flush behavior
- batching during token bursts

### 2. Streaming-safe viewport behavior
While the user is scrolled away from bottom:
- continue ingesting updates
- do not force jump to bottom
- preserve anchor / focus behavior correctly

### 3. Shared redraw behavior
Keep redraw incremental and no-flicker.
Do not reintroduce full-clear redraws for routine streaming updates.

### 4. Stay on the shared architecture
Allowed targets:
- `crates/tui/src/fullscreen/*`
- `crates/cli/src/fullscreen_entry.rs`

Forbidden targets:
- `crates/cli/src/interactive_fullscreen/*`
- `crates/cli/src/fullscreen_transcript/*`

## Verification

```bash
cd /tmp/bb-fullscreen-finish/r40-streaming
cargo build
cargo test -p bb-tui fullscreen::runtime -- --nocapture
```

## Finish

```bash
git add -A && git commit -m "finish shared fullscreen streaming scheduler"
```
