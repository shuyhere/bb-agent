# BB-Agent Full Reconstruction Plan

> 20 parallel sub-agents organized in 4 waves.
> Each wave depends on the previous wave completing.
> Within a wave, all agents run in parallel.

---

## Wave 1: Foundation (7 agents)
> Restructure crates, consolidate duplicates, build clean foundations.
> No agents in this wave depend on each other.

### Agent R1: Consolidate agent loop into core/
**Worktree:** `r1-agent-loop`
**Task:** 
- Delete `cli/src/agent_loop.rs` and `cli/src/session.rs`
- Rewrite `core/src/agent_session.rs` as THE single AgentSession:
  - Owns: Connection, session_id, model, provider, tools, settings
  - Methods: `run_prompt(text, tx)` → streams AgentLoopEvents via channel
  - Methods: `compact()`, `set_model()`, `context_usage()`
  - Auto-compaction check after each turn
  - Context overflow recovery (compact + retry)
  - Rate limit handling (wait + retry)
- Rewrite `core/src/agent_loop.rs` as internal turn loop used by AgentSession
- Delete `core/src/agent.rs` (move system_prompt to its own file)
- Create `core/src/system_prompt.rs`
- Update `core/src/lib.rs`
- All existing tests must still pass
- **Files touched:** core/src/agent_session.rs (NEW ~500 lines), core/src/agent_loop.rs (REWRITE ~300 lines), core/src/system_prompt.rs (NEW ~50 lines), delete core/src/agent.rs, delete core/src/session.rs

### Agent R2: TUI core — Component + TUI + async event loop
**Worktree:** `r2-tui-core`
**Task:**
- Rewrite `tui/src/tui.rs` — the TUI core matching pi's architecture:
  - `TUI` struct extends Container
  - Has a Terminal reference
  - `start()` — enable raw mode, start event polling
  - `stop()` — restore terminal, disable raw mode
  - `request_render()` — schedule a render on next tick
  - `set_focus(component)` — keyboard input goes to focused component
  - `do_render()` — differential rendering (compare prev vs new lines)
  - Synchronized output wrapping
- Rewrite `tui/src/terminal.rs` — proper Terminal with event polling:
  - `poll_event(timeout)` → Option<KeyEvent>
  - `write()`, `columns()`, `rows()`
  - Raw mode management
  - Drop guard for cleanup
- Rewrite `tui/src/component.rs`:
  - `Component` trait: `render(width) -> Vec<String>`, `handle_input(key)`, `invalidate()`
  - `Focusable` trait: `focused` field
  - `Container`: holds children, renders vertically
- Rewrite `tui/src/renderer.rs` — scrollback-based differential renderer:
  - Track previous lines + cursor position
  - Find first changed line, re-render from there
  - Handle width/height changes
  - Clear extra lines on shrink
- Keep `tui/src/utils.rs` (already good)
- Update `tui/src/lib.rs` with new module structure
- **Files touched:** tui/src/tui.rs (NEW ~400 lines), tui/src/terminal.rs (REWRITE ~150 lines), tui/src/component.rs (REWRITE ~100 lines), tui/src/renderer.rs (REWRITE ~200 lines)

### Agent R3: TUI components — Text, Spacer, Box, Loader, DynamicBorder
**Worktree:** `r3-tui-components`
**Task:**
- Create `tui/src/components/` directory
- Create `tui/src/components/text.rs` — static text with word wrap and padding:
  ```rust
  pub struct Text { text: String, padding_x: u16, padding_y: u16 }
  impl Component for Text { fn render(&self, width) -> Vec<String> }
  ```
- Create `tui/src/components/spacer.rs` — empty lines:
  ```rust
  pub struct Spacer(pub u16); // number of blank lines
  ```
- Create `tui/src/components/border.rs` — full-width horizontal line:
  ```rust
  pub struct DynamicBorder;
  impl Component: renders "────────" in dim color to full width
  ```
- Create `tui/src/components/loader.rs` — animated spinner:
  ```rust
  pub struct Loader { frames: &[&str], current: usize, label: String }
  ```
- Create `tui/src/components/box_component.rs` — bordered container:
  ```rust
  pub struct BoxComponent { child: Box<dyn Component>, border_color: Color }
  renders top border, child lines with │ prefix, bottom border
  ```
- Create `tui/src/components/mod.rs` — re-exports
- Add tests for each component
- **Files touched:** 7 new files in tui/src/components/ (~400 lines total)

### Agent R4: TUI editor — Bordered input component
**Worktree:** `r4-tui-editor`
**Task:**
- Rewrite `tui/src/editor.rs` as a proper Component (NOT blocking read_line):
  - Renders as a bordered box between two separator lines
  - Prompt character is `~` (like pi) not `>`
  - Multi-line text editing inside the bordered area
  - Cursor movement: arrows, Home/End, Ctrl+A/E, word-jump
  - Backspace, Delete, Ctrl+K, Ctrl+U, Ctrl+W
  - History: Up/Down when on first/last line
  - Submit: Enter (returns text via callback/channel)
  - Cancel: Escape / Ctrl+C
  - `render(width) -> Vec<String>` — renders the bordered editor
  - `handle_input(key)` — processes key events
  - Does NOT call `terminal::enable_raw_mode()` itself (TUI manages raw mode)
  - Emits a cursor marker for hardware cursor positioning
- The editor border color should indicate thinking level (like pi):
  - off = dim, low = blue, medium = cyan, high = yellow
- **Files touched:** tui/src/editor.rs (REWRITE ~600 lines)

### Agent R5: TUI footer component
**Worktree:** `r5-tui-footer`
**Task:**
- Rewrite `tui/src/footer.rs` as a Component matching pi's footer:
  - Single line at bottom of terminal
  - Layout: `cwd  ↑input ↓output $cost (sub/api)  context%/window (auto/manual)  (provider) model • thinking`
  - Colors: dim for labels, cyan for model, colored for context%
  - Truncate cwd if too long
  - Responsive: adjusts to terminal width
- Create `tui/src/footer_data.rs` — data provider:
  ```rust
  pub struct FooterData {
      pub cwd: String,
      pub model_name: String,
      pub provider: String,
      pub thinking_level: String,
      pub input_tokens: u64,
      pub output_tokens: u64,
      pub cost: f64,
      pub context_tokens: u64,
      pub context_window: u64,
      pub is_subscription: bool,
  }
  ```
- Delete old `tui/src/status.rs`
- **Files touched:** tui/src/footer.rs (REWRITE ~200 lines), tui/src/footer_data.rs (NEW ~30 lines), delete tui/src/status.rs

### Agent R6: TUI markdown — improve and wire
**Worktree:** `r6-tui-markdown`
**Task:**
- Review and improve `tui/src/markdown.rs` (currently 758 lines)
- Ensure it handles all pi markdown features:
  - Headings (bold + colored, ## level distinction)
  - Bold, italic, strikethrough
  - Code blocks with syntax highlighting (syntect) and language labels
  - Inline code with background color
  - Block quotes with │ border
  - Ordered + unordered lists (nested)
  - Links with URL display
  - Horizontal rules
  - Word wrap preserving ANSI codes
- Add a streaming-friendly mode: can re-render as text grows
- Move to `tui/src/components/markdown.rs`
- Add comprehensive tests
- **Files touched:** tui/src/components/markdown.rs (IMPROVE ~800 lines)

### Agent R7: Interactive mode components — message display
**Worktree:** `r7-interactive-messages`
**Task:**
- Create `cli/src/interactive/` directory
- Create `cli/src/interactive/components/` directory
- Create `cli/src/interactive/components/user_message.rs`:
  - Renders user message with "You" header in blue
  - Shows text content with indentation
  - Implements Component trait
- Create `cli/src/interactive/components/assistant_message.rs`:
  - Renders assistant message with "Assistant (model)" header in green
  - Uses MarkdownRenderer for text content
  - Shows thinking blocks (collapsed by default, "[thinking]" in dim)
  - Shows tool calls: `* tool_name(args_preview)`
- Create `cli/src/interactive/components/tool_execution.rs`:
  - Shows tool name + args while running
  - Shows result preview after completion
  - Expandable/collapsible (Ctrl+O toggles)
  - Green checkmark or red X for result
- Create `cli/src/interactive/components/compaction_message.rs`:
  - Shows "[c] compaction: N tokens summarized" in dim
- Create `cli/src/interactive/components/mod.rs`
- **Files touched:** 6 new files in cli/src/interactive/components/ (~500 lines total)

---

## Wave 2: Integration (5 agents)
> Wire the Wave 1 foundations together.

### Agent R8: Interactive mode controller
**Worktree:** `r8-interactive-mode`
**Task:**
- Rewrite `cli/src/interactive/mod.rs` — the main interactive controller:
  - Creates TUI with component tree:
    ```
    TUI
      ├── header_container (banner, shortcuts)
      ├── chat_container (messages — grows as conversation progresses)
      ├── DynamicBorder (separator above editor)
      ├── editor_container (the bordered editor)
      ├── DynamicBorder (separator below editor)
      └── footer
    ```
  - Async event loop:
    ```rust
    loop {
        tokio::select! {
            // Poll terminal events (keyboard input)
            key = terminal.poll_event() => handle_key(key),
            // Receive agent streaming events
            event = agent_rx.recv() => handle_agent_event(event),
        }
        tui.request_render();
    }
    ```
  - On editor submit: send text to AgentSession via channel
  - On agent events: update chat components, re-render
  - On slash commands: route to handlers
  - On Ctrl+C: abort running agent
  - Session restore: rebuild message components from context
- **Files touched:** cli/src/interactive/mod.rs (NEW ~800 lines)

### Agent R9: Wire slash commands into interactive mode
**Worktree:** `r9-slash-commands`
**Task:**
- Move `cli/src/slash.rs` → `cli/src/interactive/commands.rs`
- Wire each command to actual functionality:
  - `/model` → show ModelSelector overlay
  - `/resume` → show SessionSelector overlay  
  - `/tree` → show TreeSelector overlay
  - `/compact` → trigger compaction
  - `/new` → create new session, clear chat
  - `/name` → persist session name
  - `/session` → show session info
  - `/login` `/logout` → delegate to login module
  - `/help` → show help text
  - `/quit` → exit
- Create overlay rendering support (component shown on top of chat)
- **Files touched:** cli/src/interactive/commands.rs (REWRITE ~300 lines), cli/src/interactive/overlays.rs (NEW ~200 lines)

### Agent R10: Wire selectors as overlays
**Worktree:** `r10-selectors`
**Task:**
- Move `tui/src/model_selector.rs` → `cli/src/interactive/components/model_selector.rs`
- Move `tui/src/session_selector.rs` → `cli/src/interactive/components/session_selector.rs`
- Move `tui/src/tree_selector.rs` → `cli/src/interactive/components/tree_selector.rs`
- Each selector:
  - Is a Component
  - Captures keyboard focus when shown
  - Returns selection via callback
  - Escape cancels
- Wire into the overlay system from R9
- **Files touched:** 3 moved + modified files (~400 lines modified)

### Agent R11: Print mode cleanup
**Worktree:** `r11-print-mode`
**Task:**
- Create `cli/src/print_mode.rs` from the working parts of `cli/src/run.rs`
- Clean print mode: send prompt, stream output to stdout, exit
- Uses AgentSession from R1 (core/agent_session.rs)
- No TUI, no raw mode, just text output
- Delete `cli/src/run.rs` after extracting print mode
- Update `cli/src/main.rs` to route: interactive vs print
- **Files touched:** cli/src/print_mode.rs (NEW ~200 lines), cli/src/main.rs (MODIFY), delete cli/src/run.rs

### Agent R12: Keyboard shortcuts + keybindings
**Worktree:** `r12-keybindings`
**Task:**
- Create `tui/src/keybindings.rs`:
  - Map key combinations to actions
  - Configurable via settings.json
  - Default bindings matching pi:
    - Ctrl+C: clear/abort
    - Ctrl+D: exit (empty editor)  
    - Escape: abort running agent
    - Ctrl+P: cycle model forward
    - Shift+Tab: cycle thinking level
    - Ctrl+O: expand/collapse tool output
    - Ctrl+T: expand/collapse thinking
    - Ctrl+L: open model selector
- Wire into TUI's input handling
- **Files touched:** tui/src/keybindings.rs (NEW ~200 lines)

---

## Wave 3: Polish (5 agents)
> Enhance each component to match pi's quality.

### Agent R13: Editor autocomplete + @file
**Worktree:** `r13-autocomplete`
**Task:**
- Create `tui/src/autocomplete.rs`:
  - File path completion (scan filesystem)
  - Slash command completion
  - `@file` fuzzy file search
  - Dropdown rendered below/above cursor
- Wire into editor: Tab triggers completion, @ triggers file search
- **Files touched:** tui/src/autocomplete.rs (NEW ~300 lines), tui/src/editor.rs (MODIFY)

### Agent R14: Thinking block display
**Worktree:** `r14-thinking`
**Task:**
- In assistant_message component: show thinking blocks
- Collapsed by default: "[thinking]" in dim
- Ctrl+T toggles expansion
- When expanded: show thinking text in dim/italic
- Track collapsed/expanded state per message
- **Files touched:** cli/src/interactive/components/assistant_message.rs (MODIFY ~100 lines)

### Agent R15: Tool display improvements
**Worktree:** `r15-tool-display`
**Task:**
- Show tool args preview: `* read(path="/etc/hostname")`
- Show inline diff for edit tool results
- Show file path + line count for read results
- Ctrl+O toggles between collapsed (5-line preview) and expanded (full output)
- Spinner animation while tool is running
- **Files touched:** cli/src/interactive/components/tool_execution.rs (MODIFY ~200 lines)

### Agent R16: Header component
**Worktree:** `r16-header`
**Task:**
- Create `cli/src/interactive/components/header.rs`:
  - Shows version + shortcut hints (like pi)
  - Each shortcut on its own line, dim formatting
  - Shows loaded AGENTS.md files
  - Shows loaded plugins
  - Separator after header
- **Files touched:** cli/src/interactive/components/header.rs (NEW ~150 lines)

### Agent R17: Cost + token tracking
**Worktree:** `r17-tracking`
**Task:**
- Track cumulative cost and tokens across turns
- Update footer data after each turn
- Show ↑input ↓output in footer
- Calculate cost from model pricing (registry has cost info)
- Show (sub) or (api) based on auth type
- Show (auto) or (manual) for compaction mode
- **Files touched:** core/src/agent_session.rs (MODIFY), tui/src/footer.rs (MODIFY)

---

## Wave 4: Final integration + cleanup (3 agents)

### Agent R18: Full integration test
**Worktree:** `r18-integration`
**Task:**
- Wire everything together in cli/src/main.rs
- Test full flow: startup → banner → editor → prompt → streaming → tool calls → response
- Test /model, /resume, /help, /quit
- Test --continue (session restore)
- Test --print mode
- Test Ctrl+C abort
- Fix any compilation errors from merging all waves
- **Files touched:** cli/src/main.rs (MODIFY)

### Agent R19: Delete dead code + cleanup
**Worktree:** `r19-cleanup`
**Task:**
- Delete all unused files:
  - `cli/src/run.rs` (replaced by print_mode.rs)
  - `cli/src/agent_loop.rs` (consolidated into core/)
  - `cli/src/session.rs` (consolidated into core/)
  - `core/src/agent.rs` (split into agent_session + system_prompt)
  - `tui/src/app.rs` (replaced by tui.rs)
  - `tui/src/chat.rs` (replaced by interactive/components/)
  - `tui/src/status.rs` (replaced by footer.rs)
- Remove dead code warnings (cargo fix)
- Ensure all 120+ tests still pass
- Run `cargo clippy` and fix warnings
- **Files touched:** multiple deletes, multiple small fixes

### Agent R20: Documentation + final build
**Worktree:** `r20-docs`
**Task:**
- Update BLUEPRINT.md with final architecture
- Update README for bb-agent
- Delete AUDIT.md, PLAN.md, TUI-PLAN.md, REVIEW.md, RESTRUCTURE.md, TASK.md (consolidate into BLUEPRINT.md)
- Final `cargo build && cargo test && cargo install`
- Verify `bb --help`, `bb --list-models`, `bb login --help` all work
- **Files touched:** docs only + final verification

---

## Summary

| Wave | Agents | Purpose | Parallel? |
|------|--------|---------|-----------|
| Wave 1 | R1-R7 (7 agents) | Foundation rebuild | All parallel |
| Wave 2 | R8-R12 (5 agents) | Integration | All parallel (depends on Wave 1) |
| Wave 3 | R13-R17 (5 agents) | Polish | All parallel (depends on Wave 2) |
| Wave 4 | R18-R20 (3 agents) | Final | Sequential (depends on Wave 3) |
| **Total** | **20 agents** | | |

## Estimated new/modified lines

| Wave | New lines | Modified lines |
|------|-----------|---------------|
| Wave 1 | ~3,200 | ~500 |
| Wave 2 | ~1,700 | ~400 |
| Wave 3 | ~750 | ~300 |
| Wave 4 | ~0 | ~200 |
| **Total** | **~5,650** | **~1,400** |

Final BB-Agent would be ~16-17K lines of well-structured Rust.
