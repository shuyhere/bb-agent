# A1: Wire hook events into agent lifecycle

Working dir: `/tmp/bb-final/a1-hook-lifecycle/`
BB-Agent Rust project. Read BLUEPRINT.md and REVIEW.md for context.

## Problem
The event bus in `crates/hooks/` defines 14 event types, but NONE are ever fired from the agent loop. The extension system is architecturally complete but functionally dead.

## Task: Fire events from the agent loop at every lifecycle point

### 1. Modify `crates/cli/src/agent_loop.rs` (or wherever the agent turn loop runs)

Add event emission at each lifecycle point. The EventBus is in `crates/hooks/src/bus.rs`.

You need to pass an `Arc<EventBus>` into the agent loop and fire events:

```rust
use bb_hooks::{EventBus, Event, ToolCallEvent, ToolResultEvent, ContextEvent};
use std::sync::Arc;
```

**Events to fire (in order of the agent loop):**

1. **`session_start`** — on startup, after session is opened
2. **`before_agent_start`** — after user submits prompt, before agent loop begins
   - Payload: `{ prompt, system_prompt }`
   - Result: can inject a message, can modify system_prompt
   - If result has `system_prompt`, use it instead
   - If result has `message`, prepend it to context
3. **`turn_start`** — at the start of each LLM call
4. **`context`** — after building context messages, before calling provider
   - Payload: `{ messages: Vec<AgentMessage> }`
   - Result: if `messages` returned, use those instead
   - This is THE critical hook for context management
5. **`tool_call`** — before executing each tool call
   - Payload: `{ tool_call_id, tool_name, input }`
   - Result: if `block: true`, skip tool execution, return error result
   - If input was mutated, use mutated version
6. **`tool_result`** — after tool execution, before appending to session
   - Payload: `{ tool_call_id, tool_name, content, is_error }`
   - Result: if `content` returned, replace the tool result content
7. **`turn_end`** — after each turn completes
8. **`session_before_compact`** — before auto/manual compaction
   - Result: if `cancel: true`, skip compaction
   - If `payload` has compaction override, use that instead
9. **`session_shutdown`** — on exit

### 2. Modify `crates/hooks/src/bus.rs`

The current EventBus uses synchronous handlers (`Fn`). For the agent loop integration, we need the bus to be accessible from async code. It already uses `RwLock`, which is fine.

Make the bus `Clone`-able by wrapping in `Arc`:
```rust
pub type SharedEventBus = Arc<EventBus>;
```

### 3. Modify `crates/cli/src/interactive.rs` and `crates/cli/src/run.rs`

Pass `SharedEventBus` into the agent session and agent loop. Fire `session_start` on startup and `session_shutdown` on exit.

### 4. Apply hook results

The critical ones:

**`context` hook:**
```rust
let context_event = Event::Context(ContextEvent { messages: ctx.messages.clone() });
if let Some(result) = event_bus.emit(&context_event).await {
    if let Some(new_messages) = result.messages {
        // Deserialize and use the new messages
        ctx.messages = /* parse from JSON */;
    }
}
```

**`tool_call` hook:**
```rust
let tc_event = Event::ToolCall(ToolCallEvent {
    tool_call_id: tc.id.clone(),
    tool_name: tc.name.clone(),
    input: args.clone(),
});
if let Some(result) = event_bus.emit(&tc_event).await {
    if result.block == Some(true) {
        // Skip this tool, return error result
        let reason = result.reason.unwrap_or("Blocked by extension".into());
        // Append error tool result
        continue;
    }
}
```

### 5. Tests

Add tests in `crates/hooks/`:
```rust
#[tokio::test]
async fn test_context_hook_modifies_messages() { ... }

#[tokio::test]
async fn test_tool_call_hook_blocks() { ... }  // already exists, verify still works

#[tokio::test]
async fn test_before_agent_start_modifies_prompt() { ... }
```

### Build and test
```bash
cd /tmp/bb-final/a1-hook-lifecycle
cargo build && cargo test
git add -A && git commit -m "A1: wire all hook events into agent lifecycle"
```
