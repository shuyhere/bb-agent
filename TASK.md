# W1: Wire TUI components into AgentSession interactive loop

Working dir: `/tmp/bb-w/w1-wire-tui-session/`

## Problem
The TUI components (editor, markdown, renderer, select_list, etc.) in `crates/tui/src/` are built but NOT used. The interactive mode in `crates/cli/src/interactive.rs` exists but doesn't properly connect to the `AgentSession` in `crates/cli/src/session.rs` and the agent loop in `crates/cli/src/agent_loop.rs`.

The current `crates/cli/src/run.rs` still uses inline `print!()` for display. We need to replace this with a proper TUI-driven interactive mode.

## Task

### 1. Rewrite `crates/cli/src/interactive.rs`

Make it the REAL interactive mode that:

a) **Starts up properly:**
   - Initialize crossterm raw mode
   - Show welcome banner with version
   - Show status bar (model name, context window)
   - Show editor prompt

b) **Main loop using crossterm event polling:**
```rust
use crossterm::event::{self, Event, KeyCode, KeyModifiers, poll};

loop {
    // Check for agent events from channel (non-blocking)
    while let Ok(ev) = agent_rx.try_recv() {
        handle_agent_event(ev);
        render_all();
    }

    // Poll for keyboard input (50ms timeout)
    if poll(Duration::from_millis(50))? {
        if let Event::Key(key) = event::read()? {
            handle_key(key);
        }
    }
}
```

c) **When user submits text:**
   - If starts with `/` → route to slash command handler
   - If starts with `!` → execute bash directly, display output
   - Otherwise → spawn async task that calls agent session with streaming

d) **Display streaming agent output:**
   - On `TextDelta` → print text inline (real-time streaming)
   - On `ToolCallStart` → print `⚡ tool_name`
   - On `ToolResult` → print brief result
   - On `AssistantDone` → print newline, re-enable editor

e) **On exit (Ctrl+C/Ctrl+D):**
   - Disable raw mode
   - Show cursor
   - Print "Goodbye!"

### 2. Modify `crates/cli/src/main.rs`

Route properly:
```rust
if cli.print {
    run::run_print_mode(cli).await
} else {
    interactive::run_interactive(cli).await
}
```

### 3. Modify `crates/cli/src/run.rs`

Keep only the `run_print_mode()` function for `-p` flag. Remove all interactive code.

### 4. Key requirements
- Must use `crossterm` for terminal control (raw mode, cursor, colors)
- Must handle Ctrl+C to abort a running agent (use CancellationToken)
- Must handle terminal resize
- Must clean up terminal state on panic/exit (use a Drop guard)
- The editor from `bb_tui::editor::Editor` should be used for input, calling `read_line()`
- Status bar from `bb_tui::status::render_status()` should be displayed

### Build and test
```bash
cd /tmp/bb-w/w1-wire-tui-session
cargo build && cargo test
git add -A && git commit -m "W1: wire TUI into interactive agent loop"
```
