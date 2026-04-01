# W5: Wire slash commands + hierarchical context loading + @file args

Working dir: `/tmp/bb-w/w5-commands-context/`

## Problem
Most slash commands print a message but don't actually do anything. AGENTS.md only loads from cwd, not parent dirs. `@file` arguments don't work.

## Tasks

### 1. Wire `/model` to ModelSelector

In `crates/cli/src/interactive.rs` or wherever slash commands are handled:

```rust
SlashResult::ModelSelect(search) => {
    // Use the ModelSelector from tui
    let registry = bb_provider::registry::ModelRegistry::new();
    let models: Vec<_> = registry.list().to_vec();

    // Simple text-based selector for now
    println!("Available models:");
    for (i, m) in models.iter().enumerate() {
        let marker = if m.id == current_model_id { ">" } else { " " };
        println!("  {marker} {}. {}/{} ({}K context)", i+1, m.provider, m.id, m.context_window/1000);
    }
    print!("Select (number): ");
    // Read selection and switch model
    // Write ModelChange entry to session
}
```

### 2. Wire `/resume` to session listing + selection

```rust
SlashResult::Resume => {
    let sessions = store::list_sessions(&conn, cwd_str)?;
    if sessions.is_empty() {
        println!("No sessions found.");
    } else {
        println!("Sessions:");
        for (i, s) in sessions.iter().take(20).enumerate() {
            let name = s.name.as_deref().unwrap_or("(unnamed)");
            println!("  {}. {} {} ({} entries, {})",
                i+1, &s.session_id[..8], name, s.entry_count, s.updated_at);
        }
        print!("Select (number): ");
        // Read selection, switch to that session
        // Rebuild context and re-render messages
    }
}
```

### 3. Wire `/new` to create fresh session

```rust
SlashResult::NewSession => {
    session_id = store::create_session(&conn, cwd_str)?;
    println!("New session: {}", &session_id[..8]);
    // Clear displayed messages
}
```

### 4. Wire `/name` to persist

```rust
SlashResult::SetName(name) => {
    let entry = SessionEntry::SessionInfo {
        base: EntryBase {
            id: EntryId::generate(),
            parent_id: get_leaf(&conn, &session_id),
            timestamp: Utc::now(),
        },
        name: Some(name.clone()),
    };
    store::append_entry(&conn, &session_id, &entry)?;
    println!("Session named: {name}");
}
```

### 5. Hierarchical AGENTS.md loading

Modify the AGENTS.md loading in `crates/cli/src/run.rs` (or `core/agent.rs`):

```rust
fn load_agents_md(cwd: &Path) -> Option<String> {
    let mut contents = Vec::new();

    // 1. Global
    let global = bb_core::config::global_dir().join("AGENTS.md");
    if global.exists() {
        if let Ok(c) = std::fs::read_to_string(&global) {
            contents.push(c);
        }
    }

    // 2. Walk parent directories up to git root (or filesystem root)
    let mut dir = cwd.to_path_buf();
    let mut scanned = Vec::new();
    loop {
        let agents = dir.join("AGENTS.md");
        if agents.exists() {
            scanned.push(agents);
        }
        // Also check CLAUDE.md alias
        let claude = dir.join("CLAUDE.md");
        if claude.exists() && !agents.exists() {
            scanned.push(claude);
        }
        // Stop at git root
        if dir.join(".git").exists() {
            break;
        }
        if !dir.pop() {
            break;
        }
    }

    // Reverse so we go from root → cwd (outermost first)
    scanned.reverse();
    for path in scanned {
        if let Ok(c) = std::fs::read_to_string(&path) {
            contents.push(c);
        }
    }

    if contents.is_empty() {
        None
    } else {
        Some(contents.join("\n\n---\n\n"))
    }
}
```

### 6. Implement `@file` arguments

In `crates/cli/src/main.rs`, before routing to mode:

```rust
// Process @file arguments from messages
let mut prompt_parts = Vec::new();
let mut regular_messages = Vec::new();

for msg in &cli.messages {
    if msg.starts_with('@') {
        let path = &msg[1..];
        match std::fs::read_to_string(path) {
            Ok(content) => {
                prompt_parts.push(format!("Contents of {}:\n```\n{}\n```", path, content));
            }
            Err(e) => {
                eprintln!("Warning: Could not read {}: {}", path, e);
            }
        }
    } else {
        regular_messages.push(msg.clone());
    }
}

// Combine file contents with messages
let final_prompt = if prompt_parts.is_empty() {
    regular_messages.join(" ")
} else {
    format!("{}\n\n{}", prompt_parts.join("\n\n"), regular_messages.join(" "))
};
```

### 7. Implement `/session` info

```rust
"/session" => {
    let session = store::get_session(&conn, &session_id)?.unwrap();
    let entries = store::get_entries(&conn, &session_id)?;
    let ctx = context::build_context(&conn, &session_id)?;
    let tokens: u64 = ctx.messages.iter()
        .map(|m| compaction::estimate_tokens_text(&serde_json::to_string(m).unwrap_or_default()))
        .sum();
    println!("Session: {}", session.session_id);
    println!("  Name: {}", session.name.unwrap_or("(unnamed)".into()));
    println!("  CWD: {}", session.cwd);
    println!("  Entries: {}", entries.len());
    println!("  Context tokens: ~{}", tokens);
    println!("  Created: {}", session.created_at);
    println!("  Updated: {}", session.updated_at);
}
```

### Build and test
```bash
cd /tmp/bb-w/w5-commands-context
cargo build && cargo test
git add -A && git commit -m "W5: wire commands + hierarchical context + @file args"
```
