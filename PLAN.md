# BB-Agent: Port Progress & Execution Plan

Date: 2026-04-01
Repo: `~/BB-Agent` — single `master` branch, clean tree
Pi source: `/home/shuyhere/tmp/pi-mono`

---

# 1. Current state

## Numbers

| Metric | BB-Agent | Pi (reference) |
|--------|----------|----------------|
| Total Rust/TS lines | **23,749** | **80,562** (agent+ai+coding-agent+tui) |
| Source files | 88 `.rs` | ~117 `.ts` (coding-agent alone) |
| Crate count | 8 | 4 packages |

### Lines by crate
| Crate | Lines | Main role |
|-------|-------|-----------|
| `core` | 6,007 | Agent, agent-loop, agent-session, types, settings |
| `cli` | 5,575 | Interactive controller, run, main, components |
| `tui` | 6,583 | Terminal, editor, markdown, components, selectors |
| `session` | 1,637 | SQLite store, tree, context, compaction |
| `provider` | 1,946 | Anthropic, OpenAI, Google, registry |
| `tools` | 873 | read, bash, edit, write |
| `hooks` | 454 | Event bus |
| `plugin-host` | 674 | Plugin discovery, host, protocol |

### Key file sizes (BB vs pi)
| File | BB lines | Pi lines | Parity |
|------|----------|----------|--------|
| `agent_session.rs` / `agent-session.ts` | 1,082 | 3,059 | ~35% |
| `agent.rs` / `agent.ts` | 1,092 | 539 | structurally OK |
| `agent_loop.rs` (core) / `agent-loop.ts` | 786 | 631 | structurally OK |
| `interactive/mod.rs` / `interactive-mode.ts` | 1,614 | 4,624 | ~35% |
| `editor.rs` / `editor.ts` | 953 | 2,230 | ~43% |
| `tui_core.rs` / `tui.ts` | 123 | 1,200 | ~10% |
| `terminal.rs` / `terminal.ts` | 152 | 360 | ~42% |
| `markdown.rs` / `markdown.ts` | 758 | 824 | ~92% |
| `compaction.rs` / `compaction.ts` | 684 | 823 | ~83% |
| `settings.rs` / `settings-manager.ts` | 377 | 958 | ~39% |
| `registry.rs` / `model-registry.ts` | 385 | 788 | ~49% |
| `anthropic.rs` / `anthropic.ts` | 320 | 905 | ~35% |
| `openai.rs` / `openai-completions.ts` | 232 | 871 | ~27% |
| autocomplete | 0 | 773 | missing |
| keybindings | 0 | 244 | missing |

## Build & runtime
- `cargo build` passes (warnings only, no errors)
- `bb --help` works
- `bb` interactive startup works (renders banner, shortcuts, editor prompt, status line)
- `bb -p "..."` print mode works (hits provider, executes tools)
- Anthropic tool-call streaming verified working (BLOCK_ID_MAP fix)

## Architecture wiring
- **Print path**: `main.rs` -> `run.rs::run_print_mode()` -> `bb_core::agent_session::ThinPrintSession`
- **Interactive path**: `main.rs` -> `interactive.rs` -> `interactive/mod.rs` (controller/runtime)
- **Core runtime ports exist**: `agent_session.rs`, `agent_session_runtime.rs`, `agent_session_extensions.rs`, `agent.rs`, `agent_loop.rs`
- **Interactive controller ports exist**: `interactive_events.rs`, `interactive_commands.rs`
- **Legacy CLI ownership still exists**: `cli/agent_loop.rs` (595 lines), `cli/session.rs` (179 lines)

---

# 2. What works

- [x] CLI arg parsing, model resolution, `@file` loading
- [x] Provider streaming (Anthropic, OpenAI, Google)
- [x] Tool execution (read, write, edit, bash)
- [x] SQLite session store, tree, context builder
- [x] Compaction preparation and execution
- [x] Settings layering (global + project)
- [x] Event bus / hooks framework
- [x] Plugin discovery and host protocol
- [x] TUI primitives: component trait, container, text, spacer, box, border, loader
- [x] Editor with multiline, history, cursor movement
- [x] Markdown renderer with syntax highlighting
- [x] Interactive startup through controller/runtime path
- [x] Message display components (assistant, user, tool, bash, compaction, branch, diff)
- [x] Model/session/tree selector scaffolding
- [x] Footer/status data provider

---

# 3. What does NOT work yet

## 3A. Runtime ownership is split
`cli/agent_loop.rs` and `cli/session.rs` still hold duplicate logic that should live in `bb_core`.
The new core ports exist but are not yet the sole execution owner.

## 3B. TUI engine is underpowered
`tui_core.rs` is 123 lines vs pi's `tui.ts` at 1,200 lines.
Missing: overlay composition, focus stack, proper differential rendering.

## 3C. Interactive controller is partially wired
The controller structure exists but many behaviors are still placeholder:
- No real event subscription to `AgentSession` turn lifecycle
- Slash commands don't perform real actions (selectors not connected)
- No queue/follow-up/dequeue semantics
- No tool/thinking expand-collapse behavior

## 3D. Editor is not pi-parity
Missing: bordered input block, `@file` fuzzy insertion, autocomplete, kill ring integration, undo/redo.

## 3E. Missing pi features
- Autocomplete system (0 lines, pi has 773)
- Keybindings system (0 lines, pi has 244)
- find/grep/ls tools
- Skills/prompt templates/packages
- OAuth login flow
- HTML export
- RPC mode
- Theme engine

---

# 4. Progress estimate

| Area | Parity | Notes |
|------|--------|-------|
| Runtime/session architecture | **45%** | Structures exist, not sole owner yet |
| Interactive controller | **35%** | Scaffolded, partially active |
| TUI engine | **15%** | Primitives exist, engine too thin |
| Editor | **40%** | Functional but not pi-like |
| Providers | **45%** | 3 providers work, quirks/transforms incomplete |
| Tools | **70%** | Core 4 done, missing find/grep/ls |
| Sessions/tree/compaction | **60%** | Backend solid, interactive UX incomplete |
| Extensions/skills | **15%** | Framework only |
| **Overall** | **~35%** | |

---

# 5. Execution plan

## Wave 1: Complete runtime ownership (eliminate split)

### W1-A: Delete `cli/agent_loop.rs`, move remaining helpers to core
- Move `run_agent_loop` signature and any call-site-needed helpers to `bb_core::agent_loop`
- Make `cli/agent_loop.rs` a 5-line re-export shim or delete it
- Update `cli/run.rs` call sites

### W1-B: Delete `cli/session.rs`, move session resolution to core
- Move session-id resolution (continue/resume/ephemeral) to `bb_core::agent_session` or `bb_session`
- Make `cli/session.rs` a re-export shim or delete it

### W1-C: Unify print and interactive runtime through core
- Both modes should call the same `bb_core` session/loop layer
- `run.rs` becomes config-gathering + mode dispatch only

**Acceptance**: `cli/agent_loop.rs` and `cli/session.rs` are gone or trivial. Build passes. Both modes work.

---

## Wave 2: Wire interactive controller to real session events

### W2-A: Create a live `AgentSession` handle in interactive bootstrap
- Interactive startup creates `AgentSession` (from core)
- Session handle owns provider, model, tools, event channel

### W2-B: Connect editor submit to session turn
- Editor submit sends text to `AgentSession::submit_prompt()`
- Session runs turn through `bb_core::agent_loop`
- Events flow back to controller via channel

### W2-C: Render events as real chat components
- `AgentLoopEvent::TextDelta` -> update streaming `AssistantMessageComponent`
- `AgentLoopEvent::ToolCallStart/ToolResult` -> create/update `ToolExecutionComponent`
- `AgentLoopEvent::AssistantDone` -> finalize message

**Acceptance**: Type a prompt in interactive `bb`, get a real streamed response with tool execution displayed.

---

## Wave 3: TUI engine parity

### W3-A: Expand `tui_core.rs` to match pi's TUI
- Overlay system (show/hide, focus capture)
- Proper focus stack
- Differential rendering (compare previous vs new lines)
- Synchronized output wrapping

### W3-B: Bordered editor integration
- Editor renders inside bordered box (not plain `>` prompt)
- Border color reflects thinking level
- Cursor marker for hardware cursor positioning

### W3-C: Overlay routing for selectors
- `/model` opens real `ModelSelector` as overlay
- `/resume` opens `SessionSelector` as overlay
- `/tree` opens `TreeSelector` as overlay
- Escape dismisses overlay

**Acceptance**: Bordered editor visible. `/model` opens selector overlay. Visual output resembles pi.

---

## Wave 4: Interactive behavior depth

### W4-A: Slash command semantics
- `/new` creates new session, clears chat
- `/compact` triggers compaction
- `/name` persists session name
- `/session` shows info
- `/fork` creates branch

### W4-B: Queue and follow-up behavior
- Enter during streaming queues steering message
- Alt+Enter queues follow-up
- Escape restores queued message to editor

### W4-C: Tool/thinking display
- Ctrl+O toggles tool expansion
- Ctrl+T toggles thinking visibility
- Tool blocks stream output and show result

### W4-D: Keyboard shortcuts
- Ctrl+C abort/clear
- Ctrl+L model selector
- Ctrl+P model cycle
- Shift+Tab thinking cycle

**Acceptance**: Commands perform real actions. Queue behavior works. Keyboard shortcuts match pi.

---

## Wave 5: Editor and autocomplete parity

### W5-A: Autocomplete system
- Port `autocomplete.ts` to `tui/src/autocomplete.rs`
- `@file` fuzzy insertion
- Slash command completion
- Tab path completion

### W5-B: Editor depth
- Kill ring (Ctrl+K/Y)
- Undo/redo (Ctrl+Z/Ctrl+Shift+Z)
- Selection (Shift+arrows)
- Bracketed paste handling

### W5-C: Keybindings system
- Port `keybindings.ts` to configurable keybinding system

**Acceptance**: `@file` works. Kill ring works. All editor shortcuts match pi.

---

## Wave 6: Breadth parity

### W6-A: Missing tools
- `find.rs` (respects .gitignore)
- `grep.rs` (uses ripgrep)
- `ls.rs` (tree-like output)

### W6-B: Provider depth
- Provider-specific message transforms
- Thinking trace conversion between providers
- Mid-session model switching with context handoff

### W6-C: Compaction end-to-end
- Auto-compaction in runtime loop on overflow
- Branch summarization on tree navigation
- `/compact` fully wired

### W6-D: Extensions/skills/packages
- Extension lifecycle wiring
- Tool/command registration
- Skills loading and prompt formatting
- Prompt templates

**Acceptance**: All pi tools available. Extensions loadable. Compaction fully automatic.

---

# 6. Subagent task structure

Each wave should use **2-4 focused tmux subagents** on isolated git worktrees.

Rules for effective subagent tasks (learned from experience):
1. **Tight scope**: max 3 target files per task
2. **Explicit file list**: "Edit only: X, Y, Z"
3. **Immediate action**: "After reading, immediately edit"
4. **Build verification**: every task must end with `cargo build`
5. **No broad research**: never ask a subagent to "understand the whole codebase"
6. **Pi behavior**: every task says "preserve and port pi behavior"

Task template:
```
Worktree: /tmp/bb-wave-N/<branch>
Read: <2-3 pi files>, <2-3 BB files>
Edit only: <2-3 BB files>
Task: <one paragraph>
Verify: cargo build -p bb-core && cargo build -p bb-cli
Commit: git add <files> && git commit -m "<message>"
```

---

# 7. Recommended immediate next wave

**Wave 1** is the highest-leverage work right now because it unblocks Wave 2 (real interactive behavior).

Launch 3 subagents:
1. `w1a-delete-cli-loop` — delete/thin `cli/agent_loop.rs`
2. `w1b-delete-cli-session` — delete/thin `cli/session.rs`
3. `w1c-unify-runtime` — make print+interactive share core runtime path

After merge, immediately start Wave 2.
