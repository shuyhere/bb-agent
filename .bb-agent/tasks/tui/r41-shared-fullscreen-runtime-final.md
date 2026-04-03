# Task: r41 shared fullscreen runtime final

Worktree: `/tmp/bb-fullscreen-finish/r41-runtime`
Branch: `r41-shared-fullscreen-runtime-final`

## Goal

Finish real BB runtime mapping into the shared fullscreen transcript UI on the correct architecture.

## Starting point

Base from current `master`, then first integrate the accepted fullscreen architecture before salvaging runtime-mapping ideas.

## Mandatory first step

Bring the branch onto the accepted fullscreen base:
1. cherry-pick `c990227`
2. cherry-pick `e5796f5`

Resolve conflicts so the branch uses:
- `crates/tui/src/fullscreen/*`
- `crates/cli/src/fullscreen_entry.rs`

## Salvage source

Salvage ideas from:
- `5402cfb` from `r37-shared-fullscreen-runtime-mapping`

Port behavior only where needed. Do not preserve obsolete CLI-local fullscreen files.

## Required deliverables

### 1. Real fullscreen turn submission
`bb --fullscreen-transcript` must be able to:
- submit prompts
- trigger real BB turn execution
- keep the bottom input flow working

### 2. Transcript block mapping
Map at least:
- user messages
- assistant turn start/end
- assistant text deltas
- thinking deltas
- tool call start / args / executing / result
- status / warning / error notes

### 3. Shared transcript hierarchy
Use the shared block model to represent:
- assistant turn
  - thinking
  - tool use
    - tool result
  - assistant content

### 4. Stay on the shared architecture
Allowed targets:
- `crates/tui/src/fullscreen/*`
- `crates/cli/src/fullscreen_entry.rs`
- minimal entry plumbing only if required

Forbidden targets:
- `crates/cli/src/interactive_fullscreen/*`
- `crates/cli/src/fullscreen_transcript/*`

## Verification

```bash
cd /tmp/bb-fullscreen-finish/r41-runtime
cargo build
cargo test -p bb-tui fullscreen::runtime -- --nocapture
```

If practical, also verify the CLI help still exposes fullscreen entry correctly.

## Finish

```bash
git add -A && git commit -m "finish shared fullscreen runtime mapping"
```
