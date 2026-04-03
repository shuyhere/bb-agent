# Task: r34 fullscreen runtime mapping

Worktree: `/tmp/bb-fullscreen-next/r34-runtime`
Branch: `r34-fullscreen-runtime-mapping`

## Goal

Map BB-Agent runtime events into the shared fullscreen transcript stack.

## Required sources

- salvage integration reference: `r29-bb-integration` commit `728b53c`
- accepted/shared stack from the new fullscreen jobs

## Main deliverables

### 1. Runtime event mapping
Map at least:
- user messages
- assistant turn start/end
- assistant text deltas
- thinking deltas
- tool call start/args/executing/result
- status / warning / error notes

### 2. Shared hierarchy
Use a stable hierarchy:
- assistant turn
  - thinking
  - tool use
    - tool result
  - assistant content

### 3. Input flow
Keep prompt submission working with the fullscreen bottom-fixed editor.

### 4. Entry wiring
Keep a dedicated fullscreen flag / path.
Do not delete the old interactive mode.

### 5. Build-safe integration
The result should compile cleanly and keep the old path buildable.

## Constraints

- Do not introduce another duplicate transcript block model in CLI.
- Reuse the shared fullscreen transcript stack.
- Avoid porting the broad dirty repo changes from earlier branches.

## Verification

```bash
cd /tmp/bb-fullscreen-next/r34-runtime
cargo build
cargo test
```

## Finish

```bash
git add -A && git commit -m "map bb runtime events into fullscreen transcript stack"
```
