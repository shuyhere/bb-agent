# Fullscreen Transcript UI Gap Review

Last updated: 2026-04-03

This document supersedes the earlier gap review that was written before the latest shared-fullscreen integration pass and before the final subagent review round.

## Current architectural conclusion

The correct final architecture is now clear:

- shared fullscreen UI lives in `crates/tui/src/fullscreen/`
- BB wiring into that shared UI lives in a thin CLI adapter
- the CLI adapter should be `crates/cli/src/fullscreen_entry.rs`
- we must not grow or preserve another CLI-local fullscreen subsystem

Concretely, the old intermediate path under `crates/cli/src/interactive_fullscreen/` is not the final design and should not survive the final cutover.

## What is already on `master`

### 1. Shared fullscreen foundation
Already merged on `master`:
- alternate screen ownership
- raw mode / mouse capture shell
- fullscreen frame shell
- bottom-fixed input shell
- status line shell

Relevant commits on `master`:
- `8d5af47` `add fullscreen transcript foundation`

### 2. Structured transcript block model
Already merged on `master`:
- block ids
- block kinds
- parent / child relationships
- collapse state
- mutation helpers

Relevant commits on `master`:
- `18c071e` `add structured transcript block model`

### 3. Unified shared fullscreen baseline
Already merged on `master`:
- shared fullscreen runtime uses the structured transcript model
- shared fullscreen projection baseline exists
- shared fullscreen viewport baseline exists
- fullscreen entry foundation is no longer a flat transcript item list

Relevant commits on `master`:
- `c360a71` `unify fullscreen foundation with structured transcript state`

## What the latest review round proved

The final subagent round produced four reviewed results:

### ACCEPT
- `r35-shared-fullscreen-controls @ e5796f5`
  - transcript controls in the shared fullscreen runtime
  - keyboard controls
  - search scaffold
  - mouse wheel and click toggle
  - focused header styling
  - build + targeted tests pass

- `r38-fullscreen-cleanup @ c990227`
  - removes duplicate fullscreen integration surface
  - introduces thin `crates/cli/src/fullscreen_entry.rs`
  - tightens CLI naming around `--fullscreen-transcript`
  - build + CLI help verification pass

### SALVAGE
- `r36-shared-fullscreen-streaming @ fcb193c`
  - good scheduler / dirty-tracking / batching ideas exist
  - build + targeted tests pass
  - not mergeable as-is because it still depends on obsolete CLI-local fullscreen integration files

- `r37-shared-fullscreen-runtime-mapping @ 5402cfb`
  - good runtime-event mapping ideas exist
  - build passes
  - not mergeable as-is because the main logic still lives on the obsolete CLI-local fullscreen integration surface

## Gaps that are now closed

These are no longer open design gaps:

### Closed: shared fullscreen foundation
Already implemented on `master`.

### Closed: structured transcript block model
Already implemented on `master`.

### Closed: unified shared baseline
Already implemented on `master` by `c360a71`.

### Closed in reviewed branch: transcript controls
Implemented by accepted branch `r35`, pending integration.

### Closed in reviewed branch: fullscreen cleanup / thin entry surface
Implemented by accepted branch `r38`, pending integration.

## Remaining real gaps

The remaining work is no longer broad architecture design. It is final integration work.

### 1. Accepted branches are not integrated into `master` yet
Still needed:
- merge / cherry-pick `c990227`
- merge / cherry-pick `e5796f5`
- resolve the small conflict caused by `r35` touching a file that `r38` deletes

This is an integration task, not a design task.

### 2. Shared fullscreen streaming scheduler is not finished on `master`
Still needed on the shared stack:
- frame cadence cap
- idle flush behavior
- batched redraw during token bursts
- per-block dirty updates
- stronger no-flicker redraw path during streaming
- auto-follow behavior that stays off when the user scrolls away

The useful implementation source is:
- `r36 @ fcb193c`

But it must be ported into:
- `crates/tui/src/fullscreen/*`
- `crates/cli/src/fullscreen_entry.rs`

It must not resurrect `crates/cli/src/interactive_fullscreen/*`.

### 3. Real BB runtime event mapping is not finished on `master`
Still needed:
- prompt submission in fullscreen path triggers real BB turn execution
- user messages map into transcript blocks
- assistant text deltas map into transcript blocks
- thinking deltas map into transcript blocks
- tool-use / tool-result hierarchy maps into transcript blocks
- status / warning / error notes map cleanly into transcript blocks

The useful implementation source is:
- `r37 @ 5402cfb`

But it must be ported into:
- `crates/cli/src/fullscreen_entry.rs`
- shared fullscreen mutation helpers / runtime APIs

It must not restore another fullscreen controller subtree.

### 4. Real attached-terminal verification is still pending
Even after code integration lands, we still need direct terminal verification for:
- long streamed turns
- wheel scrolling during streaming
- click expand/collapse during streaming
- resize during streaming
- auto-follow off / on transitions
- long-session performance
- no-flicker behavior in an attached terminal

## Updated execution order

### Step 1: merge accepted work
1. `c990227` from `r38-fullscreen-cleanup`
2. `e5796f5` from `r35-shared-fullscreen-controls`

### Step 2: salvage streaming onto the shared stack
Port only the good parts from:
- `fcb193c` from `r36-shared-fullscreen-streaming`

Target only:
- `crates/tui/src/fullscreen/*`
- `crates/cli/src/fullscreen_entry.rs`

### Step 3: salvage runtime mapping onto the shared stack
Port only the good parts from:
- `5402cfb` from `r37-shared-fullscreen-runtime-mapping`

Target only:
- `crates/cli/src/fullscreen_entry.rs`
- shared fullscreen transcript mutation APIs

### Step 4: verify in a real terminal
Run:
- build/test verification
- `bb --fullscreen-transcript`
- real streaming smoke tests
- long-session behavior tests

## Final rule for all follow-up work

All remaining fullscreen work must obey this rule:

- fullscreen UI logic belongs in `crates/tui/src/fullscreen/`
- BB-specific wiring belongs in a thin `crates/cli/src/fullscreen_entry.rs`
- no new `interactive_fullscreen` or `fullscreen_transcript` subsystems should be introduced
