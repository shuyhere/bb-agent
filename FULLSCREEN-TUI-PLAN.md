# Fullscreen No-Flicker Transcript UI Plan

This plan records the new user-approved direction for BB-Agent's TUI.

## Direction change

BB-Agent's existing interactive UI is a scrollback-style renderer modeled after pi.
The new target is a fullscreen transcript UI with:

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

This work should be built in parallel on separate branches/worktrees, then merged back in waves.

## Product goal

Build a fullscreen transcript viewer for BB-Agent that feels like a live structured conversation inspector rather than a raw logging terminal.

## Non-goals for first merge wave

Do not block the entire app on a perfect migration.
The first merge wave should preserve the current interactive path while introducing a new fullscreen transcript path behind a dedicated entry switch.

## Integration strategy

1. Add a new fullscreen transcript implementation in parallel to the current interactive renderer.
2. Keep old interactive mode buildable until the new path is feature-complete.
3. Wire BB interactive events into a structured transcript state.
4. Only replace the default mode once the fullscreen path is stable.

## Parallel worktree plan

### r24-fullscreen-foundation
- Branch: `r24-fullscreen-foundation`
- Worktree: `/tmp/bb-fullscreen/r24-foundation`
- Scope:
  - alternate screen enter/leave
  - mouse capture
  - fullscreen layout shell
  - bottom-fixed input + status line frame
  - render loop skeleton

### r25-transcript-block-model
- Branch: `r25-transcript-block-model`
- Worktree: `/tmp/bb-fullscreen/r25-block-model`
- Scope:
  - structured transcript block model
  - block ids / parent-child relationships
  - collapse state
  - mutation APIs for streaming updates

### r26-projection-scroll
- Branch: `r26-projection-scroll`
- Worktree: `/tmp/bb-fullscreen/r26-projection-scroll`
- Scope:
  - visible-row projection
  - wrapping cache
  - viewport state
  - scroll behavior
  - anchor preservation on expand/collapse/resize
  - auto-follow rules

### r27-input-modes-mouse
- Branch: `r27-input-modes-mouse`
- Worktree: `/tmp/bb-fullscreen/r27-input-modes`
- Scope:
  - normal / transcript / search mode state
  - `Ctrl+O` toggle
  - transcript keyboard controls
  - mouse click hit testing
  - wheel scrolling
  - focused row / selected block behavior

### r28-streaming-scheduler
- Branch: `r28-streaming-scheduler`
- Worktree: `/tmp/bb-fullscreen/r28-streaming`
- Scope:
  - per-block streaming append
  - dirty block tracking
  - render scheduler / frame cap
  - incremental redraw
  - no-flicker behavior during token bursts

### r29-bb-integration
- Branch: `r29-bb-integration`
- Worktree: `/tmp/bb-fullscreen/r29-integration`
- Scope:
  - map BB interactive events into transcript blocks
  - wire tools / thinking / assistant content into hierarchy
  - preserve bottom input flow
  - provide a CLI switch for the new fullscreen transcript UI

## Merge order

Recommended merge order:

1. r25-transcript-block-model
2. r26-projection-scroll
3. r24-fullscreen-foundation
4. r27-input-modes-mouse
5. r28-streaming-scheduler
6. r29-bb-integration

## Verification expectations

For each branch:

```bash
cd ~/BB-Agent
cargo build
cargo test
```

For integration branch additionally verify:

```bash
cargo run -p bb-cli -- --help
cargo run -p bb-cli -- --fullscreen-transcript
```

## Launching the subagents

Use:

```bash
cd ~/BB-Agent
bash .bb-agent/tasks/tui/launch-fullscreen-subagents.sh
```

Then attach:

```bash
tmux attach -t bb-fullscreen
```
