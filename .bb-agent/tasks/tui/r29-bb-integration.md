# Task: r29 BB integration for fullscreen transcript UI

Worktree: `/tmp/bb-fullscreen/r29-integration`
Branch: `r29-bb-integration`

## Goal

Integrate the new fullscreen transcript UI into BB-Agent's existing runtime and event model.

This branch should take the new block model, projection layer, input handling, and streaming scheduler and connect them to BB's real assistant/tool/thinking flow.

## Main deliverables

### 1. Event mapping
Map BB runtime events into transcript blocks.
At minimum cover:

- user message
- assistant message
- thinking block start/update/end
- tool use start/update/end
- tool result creation/update
- status / warning / error notes

### 2. Hierarchy mapping
Use a structure like:

- assistant turn
  - thinking
  - tool use
    - tool result
  - assistant content

The exact names can vary, but the grouping should remain stable while streaming.

### 3. Input integration
Keep the BB prompt submission flow working with the new fullscreen input box.
The input must stay fixed at the bottom while transcript content scrolls above it.

### 4. Entry switch
Add a way to enter the new UI without deleting the current one.
Possible options:

- `bb --fullscreen-transcript`
- env flag
- internal option on interactive mode construction

### 5. Build-safe migration
Do not remove the old interactive path in this branch.
The new fullscreen transcript path should be selectable and buildable.

## Suggested files

- `crates/cli/src/interactive.rs`
- `crates/cli/src/run.rs`
- `crates/cli/src/main.rs`
- new fullscreen integration modules
- existing event adapters under `crates/cli/src/interactive/controller/`

## Required verification

- build passes
- existing CLI still starts
- new fullscreen mode starts
- streamed assistant output appears in structured transcript blocks
- tool execution appears in collapsible sections
- scrolling up disables auto-follow until user returns to bottom

## Verification

```bash
cd /tmp/bb-fullscreen/r29-integration
cargo build
cargo test
cargo run -p bb-cli -- --help
```

## Finish

```bash
git add -A && git commit -m "integrate fullscreen transcript UI into bb interactive runtime"
```
