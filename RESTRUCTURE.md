# BB-Agent Structure Review

## Pi's 4-package architecture

```
pi-ai/                          — LLM abstraction (26K lines)
  types.ts                      — Message, Model, Tool, Usage types
  stream.ts                     — stream() / complete() entry points
  providers/                    — 10+ provider implementations
  models.generated.ts           — full model registry
  utils/                        — OAuth, validation, JSON parse

pi-agent-core/                  — Agent loop (1.8K lines)
  types.ts                      — AgentMessage, AgentEvent, AgentTool
  agent-loop.ts                 — prompt→LLM→tools→repeat
  agent.ts                      — Agent class (state, events, queuing)

pi-tui/                         — Terminal UI framework (10.7K lines)
  terminal.ts                   — Terminal I/O abstraction
  tui.ts                        — Component tree + differential rendering
  utils.ts                      — ANSI width, truncation, wrapping
  keys.ts                       — Key sequence parsing
  components/                   — Editor, Markdown, SelectList, Box, Text, etc.

pi-coding-agent/                — The actual agent (41K lines)
  core/
    agent-session.ts            — Central session lifecycle (3K lines)
    session-manager.ts          — Append-only session store
    settings-manager.ts         — Layered settings
    model-registry.ts           — Model loading + API key resolution
    model-resolver.ts           — Fuzzy matching
    auth-storage.ts             — Auth persistence
    system-prompt.ts            — Prompt construction
    compaction/                 — Context compaction
    extensions/                 — Extension types, runner, loader
    tools/                      — read, bash, edit, write, grep, find, ls
  modes/
    interactive/
      interactive-mode.ts       — THE interactive controller (4.6K lines)
      components/               — 35 UI components for interactive mode
      theme/                    — Theme engine
    print-mode.ts               — Non-interactive mode
    rpc/                        — RPC mode
  cli/                          — Arg parsing, file processing
  utils/                        — Git, clipboard, images, etc.
```

## BB-Agent current structure (PROBLEMS)

```
core/                           — MIXED concerns
  types.rs                      ✓ good — entry/message types
  config.rs                     △ too simple — needs to be settings.rs
  settings.rs                   △ exists but duplicates config.rs
  error.rs                      ✓ good
  agent.rs                      ✗ WRONG PLACE — system prompt + helpers don't belong here
  agent_loop.rs                 ✗ WRONG PLACE — duplicates cli/agent_loop.rs
  session.rs                    ✗ WRONG PLACE — stub, duplicates cli/session.rs

session/                        ✓ GOOD — maps to pi's session-manager + compaction
  store.rs                      ✓
  schema.rs                     ✓
  tree.rs                       ✓
  context.rs                    ✓
  compaction.rs                 ✓
  import_export.rs              ✓

tools/                          ✓ GOOD — maps to pi's core/tools
  read.rs bash.rs edit.rs write.rs  ✓
  artifacts.rs                  ✓ (pi doesn't have this — BB advantage)
  diff.rs                       ✓
  scheduler.rs                  △

provider/                       △ OK but thin — maps to pi-ai
  anthropic.rs                  △ basic
  openai.rs                     △ basic
  google.rs                     △ basic
  registry.rs                   △ hardcoded models
  resolver.rs                   ✓
  retry.rs                      ✓
  streaming.rs                  ✓

hooks/                          △ maps to pi's extensions (partially)
  events.rs                     ✓
  bus.rs                        ✓

plugin-host/                    △ maps to pi's extensions/loader
  discovery.rs                  ✓
  host.rs                       △
  protocol.rs                   ✓

tui/                            ✗ BROKEN — components exist but architecture wrong
  terminal.rs                   △ basic, no event loop integration
  component.rs                  △ trait exists but not used properly
  renderer.rs                   △ fullscreen approach, should be scrollback
  editor.rs                     ✗ blocking read_line, not a component
  markdown.rs                   ✓ renders but not wired
  select_list.rs                ✓ logic works
  others...                     ✗ not wired

cli/                            ✗ MESSY — mixed concerns, duplicated code
  main.rs                       △ arg parsing
  interactive.rs                ✗ println-based, not component tree
  run.rs                        ✗ duplicates interactive.rs logic
  agent_loop.rs                 ✗ duplicates core/agent_loop.rs
  session.rs                    ✗ duplicates core/session.rs
  login.rs                      ✓
  models.rs                     ✓
  slash.rs                      ✓
```

## Key structural problems

### 1. Duplicated agent loop (3 copies!)
- `core/src/agent_loop.rs`
- `cli/src/agent_loop.rs`
- `cli/src/run.rs` (inline in run_turn)

Pi has ONE agent loop in `pi-agent-core/agent-loop.ts`.

### 2. Duplicated session handling
- `core/src/session.rs`
- `cli/src/session.rs`

Pi has ONE `AgentSession` in `coding-agent/core/agent-session.ts`.

### 3. TUI not used as a component tree
Pi's interactive mode creates a tree:
```
TUI
  ├── header
  ├── chat messages
  ├── editor (with borders)
  └── footer
```

BB's interactive mode does:
```
println!(banner)
loop {
    println!(status)
    editor.read_line()  // blocking!
    println!(messages)
}
```

This is fundamentally wrong. The editor blocks everything.

### 4. No AgentSession abstraction
Pi has `AgentSession` (3K lines) that encapsulates:
- Agent state
- Model switching
- Compaction trigger
- Extension binding
- Bash execution
- Session switching

BB spreads this across cli/run.rs, cli/interactive.rs, cli/agent_loop.rs.

## Proposed new structure

```
core/                           — Types, config, errors only
  types.rs                      — Entry/message types (keep)
  error.rs                      — Error types (keep)
  config.rs                     — Paths, dirs (keep)

session/                        — Session storage + compaction (keep as-is)
  store.rs
  schema.rs
  tree.rs
  context.rs
  compaction.rs
  import_export.rs

tools/                          — Tool implementations (keep as-is)
  read.rs bash.rs edit.rs write.rs
  artifacts.rs diff.rs scheduler.rs

provider/                       — LLM providers (keep as-is)
  anthropic.rs openai.rs google.rs
  registry.rs resolver.rs retry.rs streaming.rs

hooks/                          — Event system (keep as-is)
  events.rs bus.rs

plugin-host/                    — Plugin bridge (keep as-is)
  discovery.rs host.rs protocol.rs

agent/                          — NEW: agent loop + session lifecycle
  agent_loop.rs                 — ONE agent loop (prompt→LLM→tools→repeat)
  agent_session.rs              — AgentSession (model, compaction, extensions)
  system_prompt.rs              — Prompt construction
  settings.rs                   — Layered settings manager
  messages.rs                   — Message type helpers

tui/                            — REWRITE: component tree architecture
  terminal.rs                   — Terminal I/O (raw mode, events, sync output)
  tui.rs                        — TUI core: component tree, diff rendering, focus, overlays
  utils.rs                      — ANSI width, truncation, wrapping
  components/
    editor.rs                   — Bordered editor (the big one)
    markdown.rs                 — Markdown renderer
    text.rs                     — Static text display
    spacer.rs                   — Empty space
    box.rs                      — Bordered container
    select_list.rs              — Selectable list
    loader.rs                   — Spinner/loading indicator
    footer.rs                   — Footer bar

interactive/                    — NEW: interactive mode (like pi's modes/interactive)
  mod.rs                        — InteractiveMode controller
  components/
    assistant_message.rs        — Assistant message display (markdown + tool calls)
    user_message.rs             — User message display
    tool_execution.rs           — Tool call + result display
    compaction_message.rs       — Compaction summary
    model_selector.rs           — /model picker
    session_selector.rs         — /resume picker
    tree_selector.rs            — /tree navigator
    header.rs                   — Startup header

cli/                            — SIMPLIFIED: just entry point + arg parsing
  main.rs                       — Arg parsing + mode dispatch
  login.rs                      — Auth (keep)
  print_mode.rs                 — Non-interactive mode
```

## Changes needed

### Delete
- `core/src/agent.rs` (move to agent/)
- `core/src/agent_loop.rs` (move to agent/)
- `core/src/session.rs` (move to agent/)
- `core/src/settings.rs` (move to agent/)
- `cli/src/agent_loop.rs` (consolidate into agent/)
- `cli/src/session.rs` (consolidate into agent/)
- `cli/src/run.rs` (split into print_mode.rs + interactive/)
- `cli/src/interactive.rs` (move to interactive/)
- `cli/src/slash.rs` (move to interactive/)
- `cli/src/models.rs` (move to interactive/)

### Create
- `crates/agent/` — new crate for agent loop + session lifecycle
- `crates/interactive/` — new crate for interactive mode + components
- or fold interactive/ into tui/ to keep 8 crates

### Rewrite
- `tui/` — component tree architecture with async event loop
- Interactive mode — component-based, not println-based

## Minimum viable restructure (keep 8 crates)

If we don't want to add new crates, we can restructure within existing ones:

```
core/
  types.rs config.rs error.rs   — keep
  agent_session.rs              — NEW: consolidated AgentSession
  agent_loop.rs                 — NEW: single agent loop
  system_prompt.rs              — NEW: from agent.rs
  settings.rs                   — keep but improve

tui/
  terminal.rs                   — rewrite: async event polling
  tui.rs                        — NEW: component tree + rendering
  utils.rs                      — keep + improve
  components/                   — NEW subdirectory
    editor.rs                   — rewrite: bordered component
    markdown.rs                 — keep
    text.rs                     — NEW
    spacer.rs                   — NEW
    box_component.rs            — NEW
    select_list.rs              — keep
    footer.rs                   — from status.rs
    loader.rs                   — NEW

cli/
  main.rs                       — keep, simplify
  login.rs                      — keep
  print_mode.rs                 — from run.rs
  interactive/
    mod.rs                      — InteractiveMode (from interactive.rs)
    components/                 — mode-specific components
```

This keeps 8 crates but organizes files properly.
