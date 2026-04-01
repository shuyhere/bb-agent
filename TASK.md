# A6: Error recovery + context overflow handling + message queue

Working dir: `/tmp/bb-final/a6-error-recovery/`
BB-Agent Rust project.

## Tasks

### 1. Context overflow recovery

When the LLM returns a context overflow error (HTTP 400 with "context_length_exceeded" or similar), the agent should:

1. Detect the error
2. Auto-compact
3. Retry the request

In `crates/cli/src/agent_loop.rs` (or wherever the provider call happens):

```rust
// After provider.stream() or provider.complete():
match result {
    Err(BbError::Provider(msg)) if is_context_overflow(&msg) => {
        tracing::warn!("Context overflow detected, auto-compacting...");
        // Trigger compaction
        let path = tree::active_path(conn, session_id)?;
        if let Some(prep) = compaction::prepare_compaction(&path, settings) {
            let comp_result = compaction::compact(&prep, ...).await?;
            // Append compaction entry
            store::append_entry(conn, session_id, &comp_entry)?;
            // Retry the request (continue the loop)
            continue;
        } else {
            return Err(BbError::Provider("Context overflow but nothing to compact".into()));
        }
    }
    Err(e) => return Err(e.into()),
    Ok(events) => { /* normal flow */ }
}

fn is_context_overflow(msg: &str) -> bool {
    let msg_lower = msg.to_lowercase();
    msg_lower.contains("context_length_exceeded")
        || msg_lower.contains("maximum context length")
        || msg_lower.contains("too many tokens")
        || msg_lower.contains("request too large")
        || msg_lower.contains("prompt is too long")
        || (msg_lower.contains("400") && msg_lower.contains("token"))
}
```

### 2. Rate limit handling

When the provider returns 429 (rate limit), wait and retry:

```rust
Err(BbError::Provider(msg)) if is_rate_limited(&msg) => {
    tracing::warn!("Rate limited, waiting 10 seconds...");
    tokio::time::sleep(Duration::from_secs(10)).await;
    continue; // retry the turn
}

fn is_rate_limited(msg: &str) -> bool {
    msg.contains("429") || msg.to_lowercase().contains("rate limit")
}
```

### 3. Basic message queue

Implement simple message queuing so the user can type while the agent is working.

In `crates/cli/src/interactive.rs`:

```rust
struct MessageQueue {
    messages: Vec<QueuedMessage>,
}

enum QueuedMessage {
    Steer(String),    // Enter while agent is working
    FollowUp(String), // Alt+Enter while agent is working
}

impl MessageQueue {
    fn push_steer(&mut self, text: String) { ... }
    fn push_follow_up(&mut self, text: String) { ... }
    fn take_steers(&mut self) -> Vec<String> { ... }
    fn take_follow_ups(&mut self) -> Vec<String> { ... }
    fn is_empty(&self) -> bool { ... }
}
```

Integration:
- While agent is running, if user presses Enter → queue as steer message
- After each agent turn (before next LLM call), check for steer messages:
  ```rust
  let steers = message_queue.take_steers();
  for steer in steers {
      // Append as user message to session
      // The next LLM call will see it
  }
  ```
- After agent finishes all tool calls (AssistantDone), check for follow-ups:
  ```rust
  let follow_ups = message_queue.take_follow_ups();
  for msg in follow_ups {
      // Start a new agent turn with this message
  }
  ```

### 4. Graceful error display

Instead of crashing on provider errors, display them nicely:

```rust
Err(e) => {
    eprintln!("\x1b[31m✗ Error: {e}\x1b[0m");
    // Don't crash, re-enable editor, let user try again
    break;
}
```

### 5. Ctrl+C abort improvement

Currently Ctrl+C might not properly cancel a running agent. Ensure:
- CancellationToken is shared between input handler and agent task
- When Ctrl+C pressed during agent execution:
  1. Cancel the token
  2. Display "[Aborted]"
  3. Keep partial assistant response in session (if any)
  4. Re-enable editor

### 6. Tests

```rust
#[test]
fn test_is_context_overflow() {
    assert!(is_context_overflow("HTTP 400: context_length_exceeded"));
    assert!(is_context_overflow("maximum context length is 200000 tokens"));
    assert!(!is_context_overflow("HTTP 401: Unauthorized"));
}

#[test]
fn test_is_rate_limited() {
    assert!(is_rate_limited("HTTP 429: Rate limit exceeded"));
    assert!(!is_rate_limited("HTTP 400: Bad request"));
}

#[test]
fn test_message_queue() {
    let mut q = MessageQueue::new();
    q.push_steer("fix the bug".into());
    q.push_follow_up("then run tests".into());
    assert_eq!(q.take_steers().len(), 1);
    assert_eq!(q.take_follow_ups().len(), 1);
    assert!(q.is_empty());
}
```

### Build and test
```bash
cd /tmp/bb-final/a6-error-recovery
cargo build && cargo test
git add -A && git commit -m "A6: error recovery + context overflow + message queue"
```
