# W2: Wire compaction into agent loop + auto-compact

Working dir: `/tmp/bb-w/w2-compaction-wire/`

## Problem
Compaction logic exists in `crates/session/src/compaction.rs` (prepare + compact + serialize) but is never called. The agent loop never checks if context is too large. `/compact` command is acknowledged but doesn't execute.

## Task

### 1. Add auto-compaction to the agent loop

In `crates/cli/src/agent_loop.rs` (or `session.rs` if that's where the loop lives), after each turn:

```rust
// After appending assistant message and tool results to session
let ctx = context::build_context(&conn, &session_id)?;
let total_tokens: u64 = ctx.messages.iter()
    .map(|m| compaction::estimate_tokens_text(&serde_json::to_string(m).unwrap_or_default()))
    .sum();

if compaction::should_compact(total_tokens, model.context_window, &compaction_settings) {
    // Prepare compaction
    let path = tree::active_path(&conn, &session_id)?;
    if let Some(prep) = compaction::prepare_compaction(&path, &compaction_settings) {
        // Execute compaction (call LLM for summary)
        let result = compaction::compact(
            &prep, provider, &model.id, &api_key, &base_url, None, cancel.clone()
        ).await?;

        // Append compaction entry to session
        let comp_entry = SessionEntry::Compaction {
            base: EntryBase {
                id: EntryId::generate(),
                parent_id: get_leaf(&conn, &session_id),
                timestamp: Utc::now(),
            },
            summary: result.summary,
            first_kept_entry_id: EntryId(result.first_kept_entry_id),
            tokens_before: result.tokens_before,
            details: Some(serde_json::json!({
                "readFiles": result.read_files,
                "modifiedFiles": result.modified_files,
            })),
            from_plugin: false,
        };
        store::append_entry(&conn, &session_id, &comp_entry)?;

        println!("📦 Context compacted ({} tokens summarized)", result.tokens_before);
    }
}
```

### 2. Wire `/compact` slash command

In the slash command handler, when user types `/compact`:
- Get active path from session
- Call `prepare_compaction()` + `compact()`
- Append compaction entry
- Display confirmation

When `/compact some instructions`:
- Pass `Some("some instructions")` as custom_instructions

### 3. Handle the compaction provider dependency

The `compact()` function in `crates/session/src/compaction.rs` needs to call a provider. It currently takes provider params. Make sure it works with the actual providers (OpenAI and Anthropic).

The compaction should use a simple non-streaming request:
```rust
let events = provider.complete(request, options).await?;
let collected = CollectedResponse::from_events(&events);
let summary = collected.text;
```

### 4. Tests

Add a test that verifies auto-compaction triggers:
```rust
#[test]
fn test_should_compact_triggers() {
    let settings = CompactionSettings::default(); // reserve=16384
    assert!(should_compact(120_000, 128_000, &settings)); // over threshold
    assert!(!should_compact(100_000, 128_000, &settings)); // under threshold
}
```

### Build and test
```bash
cd /tmp/bb-w/w2-compaction-wire
cargo build && cargo test
git add -A && git commit -m "W2: wire compaction into agent loop + /compact"
```
