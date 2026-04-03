# Task: r25 transcript block model

Worktree: `/tmp/bb-fullscreen/r25-block-model`
Branch: `r25-transcript-block-model`

## Goal

Implement the structured transcript model for the new fullscreen UI.
The transcript must not be represented as one growing string.

## Main deliverables

### 1. Transcript block types
Create a transcript block model with explicit kinds.
Suggested shape:

```rust
enum BlockKind {
    UserMessage,
    AssistantMessage,
    Thinking,
    ToolUse,
    ToolResult,
    SystemNote,
}
```

Each block should carry:
- stable id
- kind
- title
- content
- collapsed flag
- expandable flag
- optional parent
- ordered children

### 2. Mutation API
Add transcript mutation helpers for:

- append root block
- append child block
- update title
- append streamed content into a block
- set collapsed / expanded state
- replace tool result content
- mark block dirty

### 3. Grouping model
Support a hierarchy like:

- assistant message
  - thinking
  - tool use
    - tool result
  - assistant content

This does not need to be the final perfect hierarchy, but the model must support it.

### 4. Storage choice
Simple `String` content is acceptable for MVP.
If you use a rope type, keep the API small and explicit.

### 5. Isolation
Keep this branch focused on the transcript domain model and tests.
Do not mix in fullscreen layout or terminal work unless required for compilation.

## Suggested locations

- `crates/tui/src/fullscreen/transcript/`
- or `crates/cli/src/interactive_fullscreen/transcript/`

## Required tests

Add tests for:

- append root and child blocks
- collapse / expand state changes
- streamed append preserves existing content
- parent-child ordering
- block lookup by id

## Constraints

- Keep types small and explicit.
- No ad-hoc flattening into one render string.
- No UI-only assumptions inside the core block definitions beyond collapse state.

## Verification

```bash
cd /tmp/bb-fullscreen/r25-block-model
cargo test
cargo build
```

## Finish

```bash
git add -A && git commit -m "add structured transcript block model"
```
