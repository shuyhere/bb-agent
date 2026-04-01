# BB-Agent: Full Change Plan

> Based on line-by-line review of
> [pi-mono/packages/coding-agent](https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent)
> (41,404 lines of TypeScript across 117 files).

## Current state

BB-Agent has **7,618 lines of Rust across 49 files**. Pi's coding-agent has
41,404 lines. The gap is ~33K lines of functionality. This plan identifies
every gap and organizes the work.

---

## Architecture comparison

### Pi's coding-agent structure

```
cli.ts / main.ts                    — CLI entry, arg parsing, session resolution
core/
  agent-session.ts           3059   — THE center of everything: agent loop, compaction,
                                      model switching, bash exec, tree nav, extension binding
  agent-session-runtime.ts    358   — Runtime bootstrap (create session, manage lifecycle)
  sdk.ts                      369   — Public SDK: createAgentSession()
  session-manager.ts         1419   — Append-only JSONL, tree, context building, branching
  settings-manager.ts         958   — Layered settings (global + project + CLI)
  model-registry.ts           788   — Model loading, custom models, API key resolution
  model-resolver.ts           628   — Fuzzy model matching, provider/id/thinking parsing
  auth-storage.ts             493   — OAuth + API key persistence
  resource-loader.ts          886   — Load extensions, skills, prompts, themes
  system-prompt.ts            161   — Build system prompt from tools + context files + skills
  bash-executor.ts            258   — Streaming bash with cancellation
  keybindings.ts              314   — Configurable key bindings
  package-manager.ts         2193   — npm/git package install/remove/update
  skills.ts                   508   — Skill loading and prompt formatting
  prompt-templates.ts         256   — Markdown template loading with args
  slash-commands.ts            52   — Built-in slash command list
  footer-data-provider.ts     339   — Footer data aggregation
  messages.ts                 133   — Message type helpers
  event-bus.ts                 30   — Simple event emitter
  diagnostics.ts               42   — Resource loading diagnostics
  defaults.ts                   3   — Default thinking level
  exec.ts                      55   — Child process exec wrapper
  output-guard.ts              43   — Stdout capture for print mode
  timings.ts                   28   — Performance timing
  source-info.ts               40   — Extension/resource provenance
  tools/
    bash.ts                   431   — Bash tool (with spawn hooks)
    edit.ts                   307   — Edit tool (multi-edit, validation)
    edit-diff.ts              445   — Diff rendering for edit results
    read.ts                   208   — Read tool (text + images)
    write.ts                  131   — Write tool
    find.ts                   314   — Find tool (respects .gitignore)
    grep.ts                   375   — Grep tool (uses ripgrep)
    ls.ts                     160   — Ls tool (tree-like output)
    truncate.ts               106   — Output truncation
    path-utils.ts              59   — Path resolution
    file-mutation-queue.ts     67   — Per-file write serialization
    render-utils.ts            71   — Tool result formatting
    tool-definition-wrapper.ts 94   — Wrap tool defs for extensions
    index.ts                  124   — Tool exports
  compaction/
    compaction.ts             823   — Core compaction logic
    branch-summarization.ts   355   — Branch summary generation
    utils.ts                  197   — Serialization, file tracking
    index.ts                   25   — Exports
  extensions/
    types.ts                 1453   — ALL extension type definitions
    runner.ts                 915   — Extension lifecycle, event dispatch
    loader.ts                 557   — Extension loading (jiti, packages)
    wrapper.ts                120   — Tool wrapping for extensions
    index.ts                   73   — Exports
  export-html/
    index.ts                  314   — HTML session export
    ansi-to-html.ts           105   — ANSI → HTML converter
    tool-renderer.ts          116   — Tool results → HTML
modes/
  interactive/
    interactive-mode.ts      4624   — Interactive TUI mode (THE big file)
    theme/theme.ts           1133   — Theme engine + JSON themes
    components/               7332   — 35 TUI components (see TUI-PLAN.md)
  print-mode.ts               219   — Non-interactive single-shot mode
  rpc/
    rpc-mode.ts               674   — JSON-RPC mode
    rpc-client.ts             505   — RPC client library
    rpc-types.ts              213   — RPC type definitions
    jsonl.ts                   34   — JSONL parsing
  index.ts                     10   — Mode exports
utils/
  git.ts                      138   — Git operations
  clipboard.ts                 60   — Clipboard integration
  clipboard-native.ts          83   — Native clipboard
  clipboard-image.ts          119   — Image clipboard
  image-resize.ts             185   — Image resizing
  image-convert.ts            119   — Image format conversion
  exif-orientation.ts         119   — EXIF orientation handling
  photon.ts                    30   — Photon WASM image processing
  frontmatter.ts               74   — YAML frontmatter parsing
  shell.ts                    117   — Shell escaping, binary sanitization
  mime.ts                      30   — MIME type detection
  sleep.ts                      3   — Async sleep
  child-process.ts             32   — Child process helpers
  changelog.ts                 65   — Changelog parsing
  tools-manager.ts             18   — Tools state
```

### BB-Agent current structure

```
core/
  types.rs              368   ✓ entry types, messages
  config.rs              78   ✓ settings, dirs
  error.rs               31   ✓ error types
  agent.rs               79   △ basic helpers, no real agent loop
session/
  store.rs              301   ✓ SQLite CRUD
  schema.rs              82   ✓ 3-table schema
  tree.rs               191   ✓ tree ops
  context.rs            287   ✓ context builder
  compaction.rs         181   △ prepare only, no execution
  import_export.rs       67   △ basic JSONL import/export
tools/
  read.rs               181   ✓ works
  bash.rs               159   △ basic, no streaming display
  edit.rs               131   ✓ works
  write.rs               82   ✓ works
  artifacts.rs          105   ✓ offload
  scheduler.rs           68   △ stub
provider/
  openai.rs             209   △ streaming works, no quirks handling
  anthropic.rs          280   △ streaming works, basic
  registry.rs           179   △ hardcoded models, no custom model loading
  streaming.rs           58   ✓ event collector
hooks/
  events.rs             118   △ event types defined
  bus.rs                175   △ basic dispatch
plugin-host/
  discovery.rs           81   ✓ scan dirs
  host.rs                68   △ spawn Node, basic
  protocol.rs            61   ✓ JSON-RPC types
tui/
  (12 files)           2655   △ components built but NOT wired into CLI
cli/
  main.rs               169   △ arg parsing
  run.rs                630   △ works but uses inline rendering, not TUI
  login.rs              242   ✓ auth storage
  models.rs              38   ✓ list models
  slash.rs              107   ✓ command parsing
```

---

## Gap analysis: what pi has that BB-Agent doesn't

### G1. Agent session lifecycle (CRITICAL)

Pi's `agent-session.ts` (3059 lines) is the core. BB-Agent has nothing equivalent.

**What it does:**
- Manages the full agent loop lifecycle
- Binds extensions to session events
- Handles auto-compaction during agent turns
- Handles model switching mid-session
- Handles bash execution with streaming
- Handles session switching (new/resume/fork)
- Handles tree navigation with branch summarization
- Manages context overflow recovery
- Tracks cost across turns
- Manages message queuing (steer + followUp)
- Coordinates compaction with hooks

**What BB needs:**
A proper `AgentSession` struct in `core/` or `session/` that manages the full lifecycle.
Currently this is all inlined in `cli/src/run.rs`.

### G2. Settings manager (HIGH)

Pi's `settings-manager.ts` (958 lines) handles layered settings.

**What BB needs:**
- Global settings (`~/.bb-agent/settings.json`)
- Project settings (`.bb-agent/settings.json`)
- Merge logic (project overrides global)
- Compaction settings
- Model defaults
- Tool configuration
- Theme selection
- Extension settings
- Package sources

Currently BB has a bare-bones `config.rs` that only loads a single file.

### G3. Model resolver (HIGH)

Pi's `model-resolver.ts` (628 lines) handles:
- `--model sonnet` → fuzzy match to `claude-sonnet-4-20250514`
- `--model anthropic/sonnet` → provider + fuzzy
- `--model sonnet:high` → model + thinking level
- `--models sonnet,gpt-4o` → Ctrl+P cycling list
- Scoped model management

BB currently does basic string splitting but no fuzzy matching.

### G4. Model registry with custom models (HIGH)

Pi's `model-registry.ts` (788 lines) handles:
- Built-in model list from `models.generated.ts`
- Custom model JSON definitions (`models.json`)
- API key resolution per provider (auth.json + env vars)
- Model validation
- Provider-specific configuration (baseUrl, headers, compat flags)
- Dynamic provider registration from extensions

BB has a hardcoded model list in `registry.rs`.

### G5. Extension system (MEDIUM — defer details, build framework)

Pi's extension system is 4 files / 3018 lines:
- `types.ts` (1453) — all types
- `runner.ts` (915) — lifecycle, dispatch
- `loader.ts` (557) — jiti loading
- `wrapper.ts` (120) — tool wrapping

**What BB needs for v1:**
- The hook dispatch already exists in `hooks/`
- Plugin host exists in `plugin-host/`
- What's missing: wiring hooks into the agent session lifecycle
- What's missing: tool registration from plugins
- What's missing: extension state management across sessions

### G6. Full compaction execution (HIGH)

Pi's compaction is 1400 lines across 4 files.

**What BB has:** `prepare_compaction()` — determines what to compact.

**What BB lacks:**
- Actually calling the LLM to generate the summary
- Split-turn handling (generate + merge two summaries)
- File operation tracking across compactions
- Auto-compact trigger during agent loop
- Manual `/compact` execution
- Conversation serialization for the LLM
- The structured summarization prompt

### G7. Project context files (MEDIUM)

Pi loads `AGENTS.md` hierarchically:
- `~/.pi/agent/AGENTS.md` (global)
- Scan parent directories up to git root
- `.pi/AGENTS.md` (project)
- Support for custom system prompt replacement

BB has basic `load_agents_md()` but no parent scanning.

### G8. Skills system (LOW — defer)

Pi's `skills.ts` (508 lines):
- Load SKILL.md files from directories
- Frontmatter parsing (name, description)
- Auto-discovery from `~/.pi/agent/skills/` and `.pi/skills/`
- Format for system prompt
- Invoke via `/skill:name` commands

### G9. Prompt templates (LOW — defer)

Pi's `prompt-templates.ts` (256 lines):
- Load markdown templates with frontmatter
- Argument substitution (`$@`, `$1`, `$2`)
- Invoke via `/template-name arg1 arg2`

### G10. Streaming bash display (HIGH)

Pi's bash execution streams output in real-time during tool execution.
BB currently waits for the process to finish, then displays output.

**What BB needs:**
- Stream stdout/stderr chunks to the TUI as they arrive
- Show a spinner/loader during execution
- Handle Ctrl+C cancellation during bash

### G11. Edit diff display (MEDIUM)

Pi's `edit-diff.ts` (445 lines) shows a colored inline diff when files are edited.

BB currently shows "Applied N/N edit(s)" with no diff.

### G12. Read-only tools: grep, find, ls (LOW)

Pi has grep (375 lines, wraps ripgrep), find (314 lines), ls (160 lines).
These are optional tools enabled with `--tools read,bash,grep,find,ls`.

### G13. Theme engine (LOW — defer)

Pi's `theme.ts` (1133 lines):
- JSON theme files
- Hot-reload on file change
- Semantic color tokens
- Theme discovery + selection

### G14. Package manager (LOW — defer)

Pi's `package-manager.ts` (2193 lines):
- `pi install npm:@foo/bar`
- `pi remove`, `pi update`, `pi list`
- npm + git package support
- Settings integration

### G15. HTML export (LOW — defer)

Pi's export-html (535 lines):
- Export session to standalone HTML file
- ANSI-to-HTML conversion
- Tool result rendering

### G16. RPC mode (LOW — defer)

Pi's RPC mode (1426 lines):
- JSON-RPC over stdio
- Full session control
- IDE integration

### G17. Interactive mode wiring (CRITICAL)

Pi's `interactive-mode.ts` (4624 lines) is the biggest file and wires everything:
- TUI setup (terminal, editor, components)
- Agent event → TUI component mapping
- Streaming assistant output → markdown display
- Tool execution → tool display component
- Slash command routing
- Keyboard shortcut handling
- Extension UI dialogs
- Session restore → re-render messages
- Model cycling (Ctrl+P)
- Thinking level cycling (Shift+Tab)
- User bash (`!command`)
- Message queuing display

BB currently has `run.rs` which does all of this inline, poorly.

---

## Priority ranking

| Priority | Gap | Why |
|----------|-----|-----|
| P0 | G17. Wire TUI into agent loop | Without this, the TUI components are useless |
| P0 | G1. Agent session lifecycle | Everything depends on this |
| P0 | G6. Full compaction execution | Agent can't handle long sessions without this |
| P0 | G10. Streaming bash display | Core UX |
| P1 | G2. Settings manager | Needed for configuration |
| P1 | G3. Model resolver (fuzzy) | Needed for `--model sonnet` |
| P1 | G4. Model registry (custom) | Needed for self-hosted models |
| P1 | G7. Project context files | Needed for per-project behavior |
| P1 | G11. Edit diff display | Expected coding agent UX |
| P2 | G5. Extension wiring | Framework exists, needs lifecycle integration |
| P2 | G12. grep/find/ls tools | Useful but bash covers it |
| P2 | G13. Theme engine | Nice to have |
| P3 | G8. Skills system | Defer |
| P3 | G9. Prompt templates | Defer |
| P3 | G14. Package manager | Defer |
| P3 | G15. HTML export | Defer |
| P3 | G16. RPC mode | Defer |

---

## Implementation plan

### Sprint 1: Agent session core (1 week)

Extract the agent loop from `cli/run.rs` into a proper `AgentSession`.

**Create `crates/core/src/session.rs`** (~500 lines):
```
AgentSession
  ├── new(config) → Self
  ├── run_prompt(text) → stream of AgentEvents
  ├── model / set_model
  ├── thinking_level / set_thinking_level
  ├── auto_compact() — check + trigger
  ├── manual_compact(instructions)
  ├── navigate_tree(target_id, options)
  ├── bind_hooks(event_bus)
  └── context_usage() → ContextUsage
```

**Create `crates/core/src/agent_loop.rs`** (~300 lines):
```
Turn loop:
  1. Build context from session
  2. Fire context hook
  3. Call provider (streaming)
  4. Parse response
  5. If tool calls:
     a. Fire tool_call hooks (can block)
     b. Execute tools
     c. Fire tool_result hooks (can modify)
     d. Append tool results to session
     e. Continue loop
  6. If no tool calls: done
  7. Check auto-compaction
```

**Modify `cli/run.rs`**: use `AgentSession` instead of inline loop.

### Sprint 2: Full compaction (0.5 week)

**Modify `crates/session/src/compaction.rs`** (~400 lines added):
- `compact()` — call LLM with structured prompt, produce summary
- `serialize_conversation()` — convert messages to text for summarizer
- Summarization prompt (the structured markdown format)
- Split-turn summary generation
- File operation extraction and tracking
- Integration with `AgentSession.auto_compact()`

### Sprint 3: Settings + model resolver (1 week)

**Create `crates/core/src/settings.rs`** (~300 lines):
- Load `~/.bb-agent/settings.json` and `.bb-agent/settings.json`
- Merge logic
- All settings: compaction, default model, tools, etc.

**Modify `crates/provider/src/registry.rs`** (~400 lines):
- Load `models.json` for custom model definitions
- Support provider-specific config (baseUrl, headers, compat)
- API key resolution: auth.json → env var → settings

**Create `crates/provider/src/resolver.rs`** (~200 lines):
- Fuzzy model matching
- Parse `provider/model:thinking` syntax
- Scoped model lists for Ctrl+P

### Sprint 4: Wire TUI into agent loop (1.5 weeks)

This is the most complex sprint. Create the interactive mode controller.

**Create `crates/cli/src/interactive.rs`** (~800 lines):
```
InteractiveMode
  ├── Setup TUI (Terminal + DiffRenderer)
  ├── Add editor component at bottom
  ├── Add footer/status bar
  ├── Main event loop:
  │   ├── Read input from editor
  │   ├── Handle slash commands
  │   ├── Handle ! bash commands
  │   ├── Send prompt to AgentSession
  │   ├── Stream events to TUI:
  │   │   ├── TextDelta → append to markdown component
  │   │   ├── ThinkingDelta → show thinking indicator
  │   │   ├── ToolCallStart → add tool component
  │   │   ├── ToolCallDelta → update tool args display
  │   │   ├── ToolResult → show result in tool component
  │   │   └── Done → re-enable editor
  │   ├── Update footer (model, tokens, cost)
  │   └── Request TUI render
  ├── Session restore (re-render messages on --continue)
  ├── Model selector (via SelectList overlay)
  ├── Session selector (via SelectList overlay)
  └── Keyboard shortcuts (Ctrl+P, Ctrl+C, etc.)
```

**Modify `crates/cli/src/main.rs`**: route to `interactive.rs` for interactive mode.

### Sprint 5: Streaming bash + edit diff (0.5 week)

**Modify `crates/tools/src/bash.rs`**:
- Add streaming callback: `on_chunk: Box<dyn Fn(&str)>`
- Send chunks to TUI in real-time
- Show spinner during execution

**Create `crates/tools/src/diff.rs`** (~200 lines):
- Generate colored inline diff for edit operations
- Show before/after with +/- lines
- Use `similar` crate

### Sprint 6: Project context + system prompt (0.5 week)

**Modify `crates/core/src/agent.rs`**:
- Hierarchical AGENTS.md loading (scan parents to git root)
- Support custom system prompt replacement directive
- Format skills section (when implemented)

### Sprint 7: Hook lifecycle wiring (0.5 week)

Wire the hook system into `AgentSession`:
- Fire `session_start` on session open
- Fire `before_agent_start` before each prompt
- Fire `turn_start`/`turn_end` around each LLM call
- Fire `tool_call` before tool execution (can block)
- Fire `tool_result` after tool execution (can modify)
- Fire `context` before LLM call (can filter messages)
- Fire `session_before_compact` before compaction (can override)
- Fire `session_shutdown` on exit

### Sprint 8: Plugin tool registration (0.5 week)

Allow TS plugins to register custom tools:
- Plugin sends `register_tool` via JSON-RPC
- Rust side creates a proxy `Tool` that calls the plugin
- Tool appears in system prompt and is callable

---

## File change list

### New files to create

| File | Est. lines | Sprint | Purpose |
|------|-----------|--------|---------|
| `core/src/session.rs` | 500 | S1 | AgentSession struct |
| `core/src/agent_loop.rs` | 300 | S1 | Turn loop logic |
| `core/src/settings.rs` | 300 | S3 | Layered settings |
| `provider/src/resolver.rs` | 200 | S3 | Fuzzy model matching |
| `cli/src/interactive.rs` | 800 | S4 | Interactive mode controller |
| `tools/src/diff.rs` | 200 | S5 | Edit diff display |
| **Total new** | **~2300** | | |

### Files to significantly modify

| File | Current | Changes | Sprint |
|------|---------|---------|--------|
| `cli/src/run.rs` | 630 | Refactor: use AgentSession, split print/interactive | S1, S4 |
| `cli/src/main.rs` | 169 | Add interactive mode routing | S4 |
| `session/src/compaction.rs` | 181 | Add full compaction execution | S2 |
| `provider/src/registry.rs` | 179 | Custom models, API key resolution | S3 |
| `provider/src/openai.rs` | 209 | Provider quirks (Groq, xAI, Cerebras) | S3 |
| `hooks/src/bus.rs` | 175 | Wire into AgentSession lifecycle | S7 |
| `tools/src/bash.rs` | 159 | Add streaming callback | S5 |
| `core/src/agent.rs` | 79 | Hierarchical AGENTS.md, system prompt | S6 |
| `tui/src/app.rs` | 65 | Wire TUI components properly | S4 |
| `core/src/config.rs` | 78 | Integrate with settings manager | S3 |

### Files that stay roughly the same

| File | Lines | Status |
|------|-------|--------|
| `core/src/types.rs` | 368 | ✓ Complete |
| `core/src/error.rs` | 31 | ✓ Complete |
| `session/src/store.rs` | 301 | ✓ Complete |
| `session/src/schema.rs` | 82 | ✓ Complete |
| `session/src/tree.rs` | 191 | ✓ Complete |
| `session/src/context.rs` | 287 | ✓ Complete |
| `session/src/import_export.rs` | 67 | ✓ Good enough |
| `tools/src/read.rs` | 181 | ✓ Complete |
| `tools/src/edit.rs` | 131 | ✓ Complete |
| `tools/src/write.rs` | 82 | ✓ Complete |
| `tools/src/artifacts.rs` | 105 | ✓ Complete |
| `provider/src/anthropic.rs` | 280 | ✓ Works |
| `provider/src/streaming.rs` | 58 | ✓ Complete |
| `hooks/src/events.rs` | 118 | ✓ Complete |
| `plugin-host/src/protocol.rs` | 61 | ✓ Complete |
| `plugin-host/src/discovery.rs` | 81 | ✓ Complete |
| `cli/src/login.rs` | 242 | ✓ Complete |
| `cli/src/models.rs` | 38 | ✓ Complete |
| `cli/src/slash.rs` | 107 | ✓ Complete |
| TUI components (12 files) | 2655 | ✓ Built by sub-agents |

---

## Sprint schedule

| Sprint | Duration | What | Estimated new/changed lines |
|--------|----------|------|---------------------------|
| S1 | 1 week | Agent session + loop | +800, ~300 modified |
| S2 | 0.5 week | Full compaction | +400, ~200 modified |
| S3 | 1 week | Settings + models | +500, ~400 modified |
| S4 | 1.5 weeks | Wire TUI | +800, ~300 modified |
| S5 | 0.5 week | Streaming bash + diff | +200, ~150 modified |
| S6 | 0.5 week | Project context | ~150 modified |
| S7 | 0.5 week | Hook lifecycle | ~200 modified |
| S8 | 0.5 week | Plugin tool registration | ~200 modified |
| **Total** | **~6 weeks** | | **+2700 new, ~1900 modified** |

After all sprints, BB-Agent would be at **~12,000 lines** — about 30% of
pi's 41K, which is expected since:
- Rust is more concise for some patterns
- We're skipping: skills, prompt templates, packages, HTML export, RPC, themes
- SQLite replaces JSONL session management code
- No image processing utils needed yet

---

## What explicitly stays deferred

| Feature | pi lines | Defer until |
|---------|---------|-------------|
| Skills system | 508 | Someone needs it |
| Prompt templates | 256 | Someone needs it |
| Package manager | 2193 | Someone needs it |
| HTML export | 535 | Someone needs it |
| RPC mode | 1426 | IDE integration needed |
| Theme engine | 1133 | JSON themes later |
| Image processing | 556 | Image support needed |
| Clipboard integration | 262 | Nice to have |
| Changelog | 65 | Not needed |
| Easter eggs | 546 | Fun but not core |
| Config selector UI | 592 | Use settings.json |
| Scoped models selector | 346 | Use --models flag |
| Session search | 194 | Large-scale search needed |

**Total deferred: ~8,612 lines** (21% of pi). These can all be added incrementally later.
