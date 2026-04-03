# Task: r32 transcript controls

Worktree: `/tmp/bb-fullscreen-next/r32-controls`
Branch: `r32-transcript-controls`

## Goal

Implement the fullscreen transcript interaction model from the plan.

## Main deliverables

### 1. UI mode state
Implement explicit modes:
- normal
- transcript
- search (scaffold acceptable)

### 2. Keyboard controls
Implement in fullscreen path:
- `Ctrl+O` toggle transcript mode
- `j` / `k`
- Up / Down
- PgUp / PgDn
- Home / End
- `g` / `G`
- `Enter` / `Space` toggle focused block
- `o` expand focused block
- `c` collapse focused block
- `/` search mode entry
- `n` / `N` search traversal scaffold

### 3. Mouse controls
Implement:
- click on action header toggles expand/collapse
- wheel scroll updates transcript viewport

### 4. Focused row styling
A focused action row must be visibly highlighted.
Keep styling terminal-friendly and lightweight.

## Required sources

- plan: `FULLSCREEN-TUI-PLAN.md`
- salvage ideas from `r26` hit testing
- ignore current `r27` branch drift inside old interactive mode

## Constraints

- Implement this in the new fullscreen path, not the old scrollback controller.
- Do not add nested scrolling inside blocks.
- Search mode can be skeletal, but the state machine must exist.

## Verification

```bash
cd /tmp/bb-fullscreen-next/r32-controls
cargo build
cargo test
```

## Finish

```bash
git add -A && git commit -m "add fullscreen transcript keyboard and mouse controls"
```
