# Fullscreen No-Flicker Transcript UI Plan

Last updated: 2026-04-03

This document records the user-approved direction for BB-Agent's TUI and the current execution plan after the latest fullscreen review round.

## Direction

BB-Agent's older interactive UI is a scrollback-style renderer modeled after pi.
The newer target is a fullscreen transcript UI with:

- alternate screen ownership
- bottom-fixed input
- transcript viewport + status line layout
- structured transcript blocks instead of flattened chat text
- expand/collapse for Thinking / Tool Use / Tool Result
- keyboard transcript mode via `Ctrl+O`
- mouse click hit testing for toggles
- wheel scrolling
- auto-follow that disables when the user scrolls away
- batched incremental rendering during streaming
- strong long-session performance

## Product goal

Build a fullscreen transcript viewer for BB-Agent that feels like a live structured conversation inspector rather than a raw logging terminal.

## Architectural conclusion

The final architecture is now clear:

### Shared TUI stack
Own all fullscreen UI behavior in:
- `crates/tui/src/fullscreen/`

This layer should contain:
- terminal ownership
- layout / frame building
- transcript block model
- projection / viewport
- controls
- streaming scheduler
- rendering

### Thin CLI adapter
Own BB-specific fullscreen bootstrapping in:
- `crates/cli/src/fullscreen_entry.rs`

This layer should:
- build the initial fullscreen config
- wire BB runtime events into shared transcript blocks
- stay thin

### Explicit non-goal
Do not preserve or introduce another fullscreen subsystem under:
- `crates/cli/src/interactive_fullscreen/`
- `crates/cli/src/fullscreen_transcript/`

## What is already complete on `master`

- fullscreen terminal shell
- bottom-fixed input shell
- status line shell
- structured transcript block model
- shared fullscreen baseline using transcript blocks
- shared projection baseline
- shared viewport baseline

Key commits already on `master`:
- `8d5af47` `add fullscreen transcript foundation`
- `18c071e` `add structured transcript block model`
- `c360a71` `unify fullscreen foundation with structured transcript state`

## Reviewed follow-up branches

### Accepted
- `r38-fullscreen-cleanup @ c990227`
  - thin shared entry surface
  - removes obsolete duplicate fullscreen surface

- `r35-shared-fullscreen-controls @ e5796f5`
  - transcript mode controls in shared fullscreen runtime
  - keyboard navigation
  - search scaffold
  - mouse wheel / click toggle
  - focused header styling

### Salvage
- `r36-shared-fullscreen-streaming @ fcb193c`
  - scheduler / dirty-tracking / batching ideas
  - not mergeable as-is because it still depends partly on obsolete CLI-local fullscreen files

- `r37-shared-fullscreen-runtime-mapping @ 5402cfb`
  - runtime-event mapping ideas
  - not mergeable as-is because the main implementation still lives on the obsolete CLI-local fullscreen surface

## Remaining execution plan

### Step 1: integrate accepted work
Merge in this order:
1. `c990227` from `r38-fullscreen-cleanup`
2. `e5796f5` from `r35-shared-fullscreen-controls`

Result:
- thin shared fullscreen entry surface
- shared fullscreen transcript controls on `master`

### Step 2: finish shared fullscreen streaming
Salvage-port the useful parts from:
- `fcb193c` from `r36-shared-fullscreen-streaming`

Target only:
- `crates/tui/src/fullscreen/*`
- `crates/cli/src/fullscreen_entry.rs`

Deliverables:
- dirty block tracking
- frame cadence cap
- idle flush behavior
- batching during token bursts
- stronger no-flicker redraw path
- correct auto-follow while user is scrolled away

### Step 3: finish BB runtime mapping
Salvage-port the useful parts from:
- `5402cfb` from `r37-shared-fullscreen-runtime-mapping`

Target only:
- `crates/cli/src/fullscreen_entry.rs`
- shared fullscreen transcript mutation APIs

Deliverables:
- fullscreen prompt submission runs real BB turns
- user / assistant / thinking / tool / result / status blocks update live in the shared fullscreen transcript

### Step 4: attached-terminal verification
Verify in a real terminal:
- long streamed turns
- scroll while streaming
- click expand/collapse while streaming
- resize while streaming
- auto-follow off / on transitions
- long-session performance
- no-flicker behavior

## Final integration strategy

1. keep the old interactive path buildable until the fullscreen path is complete
2. finish the shared fullscreen path behind `--fullscreen-transcript`
3. verify real runtime behavior and terminal feel
4. only then decide whether to make fullscreen the default path

## Canonical CLI entry

Use:
```bash
bb --fullscreen-transcript
```

Backward-compatible alias:
```bash
bb --fullscreen
```
