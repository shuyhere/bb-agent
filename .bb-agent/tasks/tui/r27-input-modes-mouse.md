# Task: r27 input modes and mouse interaction

Worktree: `/tmp/bb-fullscreen/r27-input-modes`
Branch: `r27-input-modes-mouse`

## Goal

Implement the interaction model for the new fullscreen transcript UI.
This includes normal mode, transcript mode, search mode scaffolding, keyboard bindings, focus behavior, and mouse handling.

## Main deliverables

### 1. UI modes
Add explicit mode state:

```rust
enum UiMode {
    Normal,
    Transcript,
    Search,
}
```

### 2. Transcript mode toggle
Implement `Ctrl+O` to toggle between:

- normal mode
- transcript inspection mode

### 3. Keyboard navigation
In transcript mode implement:

- `j` / Down
- `k` / Up
- `PgDn`
- `PgUp`
- `Home`
- `End`
- `g`
- `G`
- `Enter`
- `Space`
- `o`
- `c`
- `/`
- `n`
- `N`
- `Esc`

Search mode can be skeletal if needed, but the state machine should exist.

### 4. Mouse support
Add mouse interaction for:

- single click on action row toggles expand/collapse
- wheel scroll updates transcript viewport

Important:
- wheel scroll belongs to the transcript viewport
- do not add nested scroll inside blocks

### 5. Focus styling hooks
Add a way for projection/rendering to know which block row is focused so the header line can be highlighted.

## Suggested files

- `input/keyboard.rs`
- `input/mouse.rs`
- `app/modes.rs`
- `app/actions.rs`

## Required tests

Add tests for:

- `Ctrl+O` toggles modes
- transcript navigation changes focused row
- toggle commands update collapse state
- click on header toggles block
- wheel scroll moves viewport

## Constraints

- Keep input state explicit.
- Do not hide the fixed input box in normal mode.
- Search can be minimal, but the mode boundary must be clean.

## Verification

```bash
cd /tmp/bb-fullscreen/r27-input-modes
cargo test
cargo build
```

## Finish

```bash
git add -A && git commit -m "add transcript mode keyboard and mouse controls"
```
