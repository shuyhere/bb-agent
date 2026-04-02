# Task: implement auto-compaction during streaming

Worktree: `/tmp/bb-restructure/r18-auto-compact`
Branch: `r18-auto-compact`

## Goal
After each assistant response, check if context tokens exceed the threshold. If so, auto-compact before the next turn instead of letting the session die on context overflow.

## What to implement

### 1. Context threshold check
In `crates/cli/src/interactive/controller/runtime.rs`, after building the assistant message and appending it to the session (after `store::append_entry` for the assistant), add:

```rust
// Check if auto-compaction is needed
let total_tokens = collected.input_tokens + collected.output_tokens 
    + collected.cache_read_tokens + collected.cache_write_tokens;
let context_window = self.session_setup.model.context_window;
let threshold = (context_window as f64 * 0.85) as u64; // 85% threshold
if total_tokens > threshold {
    self.run_auto_compaction().await;
}
```

### 2. Auto-compaction method
Add `run_auto_compaction()` to runtime.rs or command_actions.rs:

1. Show status: "Auto-compacting context..."
2. Read all entries from current branch
3. Call the compaction summarizer (use the existing `bb_session::compaction` module)
4. Build a summary using the current model (send a compaction prompt to the provider)
5. Save compaction entry to session
6. Show status: "Context compacted: 150k -> 12k tokens"
7. Continue the turn loop with compacted context

### 3. Context overflow detection
Also detect context overflow from provider error messages:
- "context_length_exceeded"
- "maximum context length"
- "too many tokens"
- "prompt is too long"

When detected:
1. Remove the failed assistant message
2. Auto-compact
3. Retry the turn

### 4. Use existing compaction infrastructure
BB already has:
- `crates/session/src/compaction/` — planning, serialization, summarization
- `crates/core/src/agent_loop/compat.rs` — `is_context_overflow()`
- `crates/cli/src/interactive/controller/command_actions.rs` — `handle_compact_command()`

Wire the auto-compact to use the same compaction logic as `/compact`, but triggered automatically.

## Constraints
- Don't compact if already compacted recently (check last entry type)
- Show clear feedback during compaction
- Esc should cancel auto-compaction
- Don't compact on aborted responses

## Verification
```
cargo build -q
```

## Finish
```
git add -A && git commit -m "implement auto-compaction on context threshold and overflow"
```
