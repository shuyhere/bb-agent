# W3: Apply --thinking, --tools, --session, abort, and model switching

Working dir: `/tmp/bb-w/w3-apply-flags/`

## Problem
Several CLI flags are parsed but not applied. Abort doesn't work. Model changes aren't persisted.

## Tasks

### 1. Apply `--thinking` to provider requests

In `crates/cli/src/run.rs` and wherever CompletionRequest is built:
- Parse thinking level from CLI
- For Anthropic: add `thinking` parameter to request body in `crates/provider/src/anthropic.rs`
  ```rust
  // In the request body building:
  if let Some(thinking) = &request.thinking {
      body["thinking"] = json!({
          "type": "enabled",
          "budget_tokens": match thinking.as_str() {
              "low" => 2048,
              "medium" => 8192,
              "high" => 16384,
              _ => 8192,
          }
      });
  }
  ```
- For OpenAI: set `reasoning_effort` parameter

Add `thinking: Option<String>` to `CompletionRequest` in `crates/provider/src/lib.rs`.

### 2. Apply `--tools` filtering

In `crates/cli/src/run.rs`:
```rust
let tool_names: Vec<&str> = if cli.no_tools {
    vec![]
} else if let Some(tools_str) = &cli.tools {
    tools_str.split(',').map(|s| s.trim()).collect()
} else {
    vec!["read", "bash", "edit", "write"]
};

let tools: Vec<Box<dyn Tool>> = builtin_tools()
    .into_iter()
    .filter(|t| tool_names.contains(&t.name()))
    .collect();
```

### 3. Implement abort with CancellationToken

In the agent loop:
- Create a `CancellationToken` shared between the agent task and input handler
- When Escape or Ctrl+C is pressed during agent execution:
  ```rust
  cancel_token.cancel();
  ```
- The provider's streaming loop checks `cancel.is_cancelled()` and stops
- Tool execution checks the cancel token too
- Display "[Aborted]" when cancelled
- Re-enable the editor

### 4. Write model_change entry on `/model` switch

When the user switches models (via `/model` or Ctrl+P):
```rust
let entry = SessionEntry::ModelChange {
    base: EntryBase {
        id: EntryId::generate(),
        parent_id: get_leaf(&conn, &session_id),
        timestamp: Utc::now(),
    },
    provider: new_model.provider.clone(),
    model_id: new_model.id.clone(),
};
store::append_entry(&conn, &session_id, &entry)?;
```

### 5. Implement `--session <id>` resolution

When `--session` is provided:
- If it looks like a file path → use directly
- Otherwise → search session IDs in the database that start with the given prefix
- Open that session instead of creating a new one

```rust
if let Some(session_arg) = &cli.session {
    // Try to find by prefix
    let all_sessions = store::list_sessions(&conn, cwd_str)?;
    let matches: Vec<_> = all_sessions.iter()
        .filter(|s| s.session_id.starts_with(session_arg))
        .collect();
    match matches.len() {
        1 => session_id = matches[0].session_id.clone(),
        0 => anyhow::bail!("No session matching '{session_arg}'"),
        n => anyhow::bail!("{n} sessions match '{session_arg}', be more specific"),
    }
}
```

### 6. Implement piped stdin

```rust
// In main.rs, before routing to mode:
let stdin_content = if !std::io::stdin().is_terminal() {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    if buf.trim().is_empty() { None } else { Some(buf) }
} else {
    None
};

// Prepend to prompt if available
if let Some(stdin) = stdin_content {
    prompt = format!("{}\n\n{}", stdin, prompt);
}
```

Add `use std::io::{IsTerminal, Read};` at top.

### Build and test
```bash
cd /tmp/bb-w/w3-apply-flags
cargo build && cargo test
git add -A && git commit -m "W3: apply --thinking, --tools, abort, model switch, piped stdin"
```
