# Task: r30 unify fullscreen stack

Worktree: `/tmp/bb-fullscreen-next/r30-unify`
Branch: `r30-unify-fullscreen-stack`

## Goal

Build the shared fullscreen transcript stack that combines the good pieces from the earlier branches without carrying over the off-scope drift.

## Required sources

Use these as references:

- accepted foundation: `r24-fullscreen-foundation` commit `9a2a555`
- accepted transcript model: `r25-transcript-block-model` commit `ebbc4c4`
- salvage references only:
  - `r26-projection-scroll` commit `6eb6005`
  - `r29-bb-integration` commit `728b53c`

## Main deliverables

### 1. Create one shared fullscreen transcript module tree
Target shared code under `crates/tui/src/fullscreen/`.

The fullscreen stack should have explicit modules for:
- transcript state
- projection integration point
- viewport state
- runtime shell
- rendering shell

### 2. Keep the accepted foundation
Retain:
- alternate screen ownership
- mouse capture
- fullscreen event loop shell
- bottom-fixed input shell
- status line shell

### 3. Replace plain transcript items with structured transcript blocks
Foundation must stop using plain transcript item strings and instead use the structured transcript block model from `r25`.

### 4. Avoid duplicate block models
Do not add another transcript block definition in CLI.
The shared fullscreen transcript state should live in one place.

### 5. Build safety
Keep old interactive mode untouched.
The fullscreen path stays parallel and buildable.

## Constraints

- Do not port the giant dirty repo-wide diffs from `r25` or `r26`.
- Keep the work focused on the shared fullscreen stack only.
- Prefer small module files; keep `mod.rs` as router only.

## Verification

```bash
cd /tmp/bb-fullscreen-next/r30-unify
cargo build
```

## Finish

```bash
git add -A && git commit -m "unify fullscreen foundation with structured transcript stack"
```
