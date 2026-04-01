# Sprint 4: Wire TUI Components into Agent Loop

You are working in a git worktree at `/tmp/bb-worktrees/s4-wire-tui/`.
This is the BB-Agent project — a Rust coding agent. Read `BLUEPRINT.md`, `PLAN.md`, and `TUI-PLAN.md` for context.

## Your task

Create an interactive mode controller that uses the TUI components (already built in
`crates/tui/src/`) to provide a proper terminal UI. The TUI components exist but are
NOT wired into the CLI's main loop — the CLI currently uses inline `print!()` statements.

### 1. Create `crates/cli/src/interactive.rs` (~800 lines)

This is the interactive mode controller — the equivalent of pi's `interactive-mode.ts`.

```rust
use bb_tui::terminal::{ProcessTerminal, Terminal};
use bb_tui::renderer::DiffRenderer;
use bb_tui::component::Container;
use bb_tui::editor::Editor;
use bb_tui::chat;
use bb_tui::markdown::MarkdownRenderer;
use bb_tui::status;
use bb_tui::select_list::SelectList;
use bb_tui::model_selector::ModelSelector;
use bb_tui::session_selector::SessionSelector;

pub struct InteractiveMode {
    terminal: ProcessTerminal,
    renderer: DiffRenderer,
    editor: Editor,
    messages: Vec<RenderedMessage>,
    model_name: String,
    total_tokens: u64,
    context_window: u64,
    total_cost: f64,
}

enum RenderedMessage {
    User(Vec<String>),
    Assistant(Vec<String>),   // pre-rendered markdown lines
    ToolResult(Vec<String>),
    Compaction(Vec<String>),
    Streaming(Vec<String>),   // currently-streaming assistant message
}
```

#### Startup flow
1. Create `ProcessTerminal` and enable raw mode
2. Create `DiffRenderer`
3. Print welcome banner
4. Print status bar (model, context window)
5. Show editor prompt at bottom
6. Enter main event loop

#### Main event loop
```rust
loop {
    // Render: collect all message lines + status + editor → send to DiffRenderer
    self.render();

    // Wait for input event (key press or agent event)
    match event {
        EditorSubmit(text) => {
            if text.starts_with('/') → handle slash command
            if text.starts_with('!') → run bash directly
            else → send prompt to agent session
        }
        AgentEvent(event) => {
            match event {
                TextDelta { text } → append to streaming markdown
                ThinkingDelta { text } → show thinking indicator
                ToolCallStart { name, .. } → show tool name
                ToolExecuting { name, .. } → show spinner
                ToolResult { .. } → show result preview
                AssistantDone → finalize message, re-enable editor
                Error { .. } → show error
            }
            self.render();  // re-render after each event
        }
        Resize → self.render()
        Ctrl+C → abort current operation
        Ctrl+D → exit
        Ctrl+P → show model selector overlay
    }
}
```

#### Render function
```rust
fn render(&mut self) {
    let width = self.terminal.columns();
    let mut lines: Vec<String> = Vec::new();

    // 1. All chat messages
    for msg in &self.messages {
        lines.extend(msg.lines());
    }

    // 2. Status bar
    lines.push(status::render_status(...));

    // 3. Editor
    lines.extend(self.editor.render(width));

    // 4. Send to differential renderer
    self.renderer.render(&lines, &mut self.terminal);
}
```

#### Slash command handling
Route to appropriate handler:
- `/model` → show `ModelSelector` (use `select_list`)
- `/resume` → show `SessionSelector`
- `/compact` → call agent session compact
- `/tree` → show tree (text for now)
- `/login` / `/logout` → call login module
- `/new` → create new session
- `/help` → display help
- `/quit` → exit

#### Agent event display
When the agent is running:
- Add a `RenderedMessage::Streaming` entry
- On each `TextDelta`, append text to the streaming buffer
- Re-render the markdown for the streaming content
- On `ToolCallStart`, show `⚡ tool_name` line
- On `ToolResult`, show brief result preview (5 lines)
- On `AssistantDone`, convert streaming to final `Assistant` message

#### Session restore
When `--continue` is used:
- Load messages from session context
- Render each message into `RenderedMessage` entries
- Display them before showing editor

### 2. Modify `crates/cli/src/main.rs`

Add interactive mode routing:
```rust
if cli.print {
    run::run_print_mode(cli).await
} else {
    interactive::run_interactive(cli).await
}
```

### 3. Modify `crates/cli/src/run.rs`

Rename/refactor the existing `run_agent` to `run_print_mode` for print mode only.
The interactive path should go through `interactive.rs`.

### 4. Handle keyboard input properly

Use crossterm's event polling in the main loop:
```rust
use crossterm::event::{self, Event, KeyCode, KeyModifiers, poll};
use std::time::Duration;

// Poll for input events with timeout
if poll(Duration::from_millis(50))? {
    if let Event::Key(key) = event::read()? {
        // Handle key
    }
}

// Also check for agent events from channel
if let Ok(agent_event) = rx.try_recv() {
    // Handle agent event
}
```

### 5. Model selector integration

When user types `/model`:
1. Build list of models from registry
2. Create `ModelSelector` 
3. Enter selection loop (the selector handles its own key events)
4. On selection: update agent session model
5. Re-render status bar

### Important notes

- Do NOT use `ratatui`. Use crossterm directly + the existing TUI components.
- The `DiffRenderer` in `tui/renderer.rs` handles differential rendering.
- The `Editor` in `tui/editor.rs` handles user input.
- Wrap all terminal output in synchronized output (`\x1b[?2026h` / `\x1b[?2026l`).
- Clean up terminal on exit (disable raw mode, show cursor).
- Handle Ctrl+C gracefully (abort current operation, don't crash).

## Build and test

```bash
cd /tmp/bb-worktrees/s4-wire-tui
cargo build
cargo test
```

Make sure ALL existing tests still pass. Then commit:
```bash
git add -A && git commit -m "S4: wire TUI components into interactive mode"
```
