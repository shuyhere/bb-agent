# BB-Agent Review: How Far From a Rust-Native Extensible Agent?

> Comparing BB-Agent (10,921 lines of Rust, 57 files) against pi-mono
> (101,004 lines of TypeScript, 197 files across 4 packages).

---

## Pi-mono architecture (what we're measuring against)

Pi is 4 packages:

| Package | Lines | Files | What it does |
|---------|-------|-------|-------------|
| `pi-ai` | 26,575 | 43 | Unified LLM API: 10+ providers, streaming, tool calling, context handoff, OAuth, model registry |
| `pi-agent-core` | 1,859 | 5 | Agent loop: prompt → LLM → tools → repeat, with event streaming |
| `pi-tui` | 10,724 | 25 | Terminal framework: differential rendering, editor, markdown, select list, overlays |
| `pi-coding-agent` | 41,404 | 124 | The coding agent: session manager, compaction, extensions, tools, interactive mode, settings |
| **Total** | **80,562** | **197** | (excluding 14K of auto-generated models) |

BB-Agent maps to this as:

| BB-Agent crate | Pi equivalent | BB lines | Pi lines | Coverage |
|----------------|--------------|----------|----------|----------|
| `core` | `pi-agent-core` + parts of `pi-coding-agent/core` | 973 | ~5,000 | ~20% |
| `session` | `pi-coding-agent/core/session-manager` + `compaction` | 1,637 | ~2,800 | ~58% |
| `tools` | `pi-coding-agent/core/tools` | 873 | ~2,500 | ~35% |
| `provider` | `pi-ai` | 1,195 | ~26,500 | ~5% |
| `hooks` | `pi-coding-agent/core/extensions` | 298 | ~3,000 | ~10% |
| `plugin-host` | `pi-coding-agent/core/extensions/loader` | 223 | ~550 | ~40% |
| `tui` | `pi-tui` | 2,655 | ~10,700 | ~25% |
| `cli` | `pi-coding-agent/modes` + `main.ts` + `cli/` | 3,067 | ~10,000 | ~30% |
| **Total** | | **10,921** | **~61,000** | **~18%** |

---

## Layer-by-layer gap analysis

### 1. LLM Provider Layer (`provider` → `pi-ai`)

**BB-Agent: 1,195 lines / Pi: 26,575 lines = 5% coverage**

This is the **biggest gap**.

| What pi-ai has | BB status |
|----------------|-----------|
| Anthropic Messages API (905 lines) | △ Basic (280 lines). Missing: thinking budget, adaptive thinking, cache control, signed blob handling, tool choice, copilot stealth mode |
| OpenAI Completions API (871 lines) | △ Basic (209 lines). Missing: provider quirks (Cerebras, xAI, Mistral, Chutes), reasoning_effort, developer role |
| OpenAI Responses API (929 lines) | ✗ Not implemented |
| OpenAI Codex Responses (929 lines) | ✗ Not implemented |
| Google Generative AI (476 lines) | ✗ Not implemented |
| Google Vertex (542 lines) | ✗ Not implemented |
| Google Gemini CLI (987 lines) | ✗ Not implemented |
| Amazon Bedrock (807 lines) | ✗ Not implemented |
| Mistral (585 lines) | ✗ Not implemented |
| Azure OpenAI Responses (147 lines) | ✗ Not implemented |
| Faux/mock provider (498 lines) | ✗ Not implemented |
| Provider registry + dispatch (433 lines) | △ Basic dispatch by ApiType enum |
| Message transformation (201 lines) | ✗ Not implemented (inline in each provider) |
| Context handoff between providers | ✗ Not implemented |
| OAuth flows (5 providers, ~2,500 lines) | ✗ Only API key auth |
| Model registry (14,002 auto-gen + 169 manual) | △ 10 hardcoded models |
| Streaming event types + EventStream | △ Basic StreamEvent enum |
| Tool argument validation (TypeBox + AJV) | ✗ Not implemented |
| Abort with partial results | △ CancellationToken exists, partial results not preserved |
| Cost/token tracking | △ Basic Usage struct, no cost calculation |
| Retry with exponential backoff | ✗ Not implemented |

**Bottom line:** BB-Agent can talk to Anthropic and OpenAI-compatible endpoints. That covers the most important use cases. But it lacks: Google, Bedrock, OAuth subscriptions, context handoff, provider quirks, retry logic, and the full model registry. The provider layer needs the most work for real-world robustness.

### 2. Agent Loop (`core` → `pi-agent-core`)

**BB-Agent: 973 lines / Pi: 1,859 lines = 52% coverage**

| What pi-agent-core has | BB status |
|------------------------|-----------|
| Agent loop (prompt → LLM → tools → repeat) | ✓ Works |
| Event streaming (AgentEvent types) | ✓ Works |
| Tool argument validation before execution | ✗ Not implemented |
| Sequential + parallel tool execution modes | △ Sequential only |
| `beforeToolCall` / `afterToolCall` callbacks | △ Event bus exists but not wired into loop |
| Message queuing (steer + followUp) | ✗ Not implemented |
| Agent state management | △ Basic |
| Proxy mode (for remote execution) | ✗ Not implemented |
| Error recovery (context overflow → compact → retry) | ✗ Not implemented |

**Bottom line:** The core agent loop works. Missing: parallel tools, message queuing, error recovery, and proper hook integration within the loop itself.

### 3. Session Engine (`session` → `pi-coding-agent/core/session-manager` + `compaction`)

**BB-Agent: 1,637 lines / Pi: ~2,800 lines = 58% coverage**

| What pi has | BB status |
|-------------|-----------|
| Append-only entry storage | ✓ SQLite (better than pi's JSONL) |
| Tree structure (id/parentId) | ✓ Works, tested |
| Context building from active path | ✓ Works, tested |
| Compaction boundary handling | ✓ Works, tested |
| Branch management (branch, resetLeaf) | ✓ Works |
| Compaction preparation | ✓ Works |
| Compaction execution (LLM summary) | ✓ Implemented |
| Conversation serialization | ✓ Implemented |
| File operation tracking | ✓ Implemented |
| Split-turn compaction | △ Flag exists, dual-summary not fully tested |
| Branch summarization | △ Logic exists, not wired to /tree |
| Auto-compaction trigger | ✓ In agent loop |
| Session migration (v1→v2→v3) | N/A (BB uses own schema) |
| JSONL import/export | ✓ Basic |
| Session listing by cwd | ✓ Works |
| Session naming | ✓ Works |
| Labels | ✗ Entry type exists, no UI |
| Fork to new session file | ✗ Not implemented |

**Bottom line:** The session engine is the **strongest part** of BB-Agent. SQLite storage is arguably better than pi's JSONL for scale. The core session operations work well.

### 4. Tools (`tools` → `pi-coding-agent/core/tools`)

**BB-Agent: 873 lines / Pi: ~2,500 lines = 35% coverage**

| What pi has | BB status |
|-------------|-----------|
| read tool | ✓ Works (images, offset/limit) |
| bash tool | ✓ Works (streaming, timeout, cancel) |
| edit tool | ✓ Works (exact text replacement) |
| write tool | ✓ Works |
| grep tool (ripgrep wrapper, 375 lines) | ✗ Not implemented |
| find tool (gitignore-aware, 314 lines) | ✗ Not implemented |
| ls tool (tree output, 160 lines) | ✗ Not implemented |
| Edit diff rendering (445 lines) | ✓ Implemented |
| File mutation queue (67 lines) | △ Struct exists, not used |
| Truncation (106 lines) | ✓ Via artifacts |
| Path utils (59 lines) | △ Basic |
| Tool definition wrapper for extensions | ✗ Not implemented |
| Artifact offload | ✓ Works (pi doesn't have this!) |

**Bottom line:** The 4 core tools work. Artifact offload is a BB-Agent advantage. Missing: grep/find/ls (optional tools), file mutation queue integration, and tool definition wrapper for extensions.

### 5. Extension/Hook System (`hooks` + `plugin-host` → `pi-coding-agent/core/extensions`)

**BB-Agent: 521 lines / Pi: ~3,000 lines = 17% coverage**

| What pi has | BB status |
|-------------|-----------|
| Event bus with typed events | ✓ Works, tested |
| Handler registration | ✓ Works |
| Block/cancel/modify semantics | ✓ Works |
| Result merging across handlers | ✓ Works |
| Extension loading (jiti/TypeScript) | △ Node spawn exists, no actual loading |
| Extension factory function | ✗ Not implemented |
| `pi.registerTool()` | ✗ Not implemented |
| `pi.registerCommand()` | ✗ Not implemented |
| `pi.registerShortcut()` | ✗ Not implemented |
| `pi.sendMessage()` | ✗ Not implemented |
| `pi.appendEntry()` | ✗ Not implemented |
| `pi.on()` wired to agent lifecycle | ✗ Events defined but not fired from agent loop |
| `ctx.ui.*` dialogs | ✗ Not implemented |
| `ctx.sessionManager` read access | ✗ Not implemented |
| Extension error handling | ✗ Not implemented |
| Extension hot reload | ✗ Not implemented |
| Custom tool rendering | ✗ Not implemented |
| Plugin JSON-RPC protocol | ✓ Types defined |
| Plugin discovery | ✓ File scanning works |

**Bottom line:** The foundation exists (event bus, plugin host, discovery) but nothing is wired. The extension system is the component that most separates pi from simpler agents. BB-Agent has the architecture for it but none of the lifecycle integration.

### 6. TUI (`tui` → `pi-tui`)

**BB-Agent: 2,655 lines / Pi: 10,724 lines = 25% coverage**

| What pi-tui has | BB status |
|-----------------|-----------|
| Terminal abstraction | ✓ ProcessTerminal |
| Component trait | ✓ Component + Container |
| Differential renderer | ✓ DiffRenderer |
| Synchronized output | ✓ In terminal |
| ANSI-aware width/truncation | ✓ utils.rs |
| Editor (2,230 lines) | △ 508 lines. Missing: selection, kill ring, undo, autocomplete, paste handling, @file |
| Markdown (824 lines) | ✓ 769 lines. Works |
| SelectList (229 lines) | ✓ 383 lines. Works |
| Input (503 lines) | ✗ Not implemented (editor covers basic input) |
| Box/bordered container (137 lines) | ✗ Not implemented |
| Image display (104 lines) | ✗ Not implemented |
| Loader/spinner (55 lines) | ✗ Not implemented |
| Overlay system | ✗ Not implemented |
| Focus management | ✗ Not implemented |
| Cursor marker (IME) | ✗ Not implemented |
| Kitty keyboard protocol | ✗ Not implemented (crossterm handles keys) |
| Bracketed paste | ✗ Not implemented |
| Key sequence parser (1,356 lines) | ✗ Using crossterm's event system instead |
| Autocomplete (773 lines) | ✗ Not implemented |
| Fuzzy matching (133 lines) | ✓ In resolver |
| Text/Spacer/TruncatedText | ✗ Not implemented |

**Bottom line:** The core rendering infrastructure works. The editor needs significant expansion. Overlays, image support, and advanced input handling are missing.

### 7. Interactive Mode (`cli` → `pi-coding-agent/modes/interactive`)

**BB-Agent: 3,067 lines / Pi: ~12,000 lines = 26% coverage**

| What pi's interactive mode has | BB status |
|-------------------------------|-----------|
| Main event loop | ✓ Works |
| Agent event → TUI component mapping | △ Basic inline display |
| Streaming text display | ✓ Works |
| Tool execution display | △ Brief preview only |
| Slash command routing | ✓ Works |
| Keyboard shortcut handling | △ Basic (Ctrl+C, Ctrl+D) |
| Model selector UI | △ Built, partially wired |
| Session selector UI | △ Built, partially wired |
| Tree selector UI (1,239 lines) | ✗ Not implemented |
| Settings selector (432 lines) | ✗ Not implemented |
| Footer (220 lines) | △ Status rendering exists |
| Session restore (re-render on --continue) | △ Loads messages, basic render |
| Message queue management | ✗ Not implemented |
| Extension UI integration | ✗ Not implemented |
| Theme loading | ✗ Not implemented |
| 35 specialized components | Only 5 basic ones |

**Bottom line:** The interactive mode works for basic coding sessions. The main gaps are: no tree selector, no overlays, basic tool display, no message queue, and no extension UI.

---

## What BB-Agent has that Pi doesn't

| BB-Agent advantage | Detail |
|-------------------|--------|
| **SQLite session storage** | Indexed, queryable, scales to millions of entries. Pi uses flat JSONL. |
| **Artifact offload** | Large tool outputs saved to disk with truncated previews. Pi keeps everything inline. |
| **Rust performance** | ~14MB binary, fast startup, low memory. Pi is Node.js. |
| **Single binary** | No Node.js dependency for the core agent. |

---

## Honest assessment: how far away?

### For daily use as a coding agent: **~70% there**

The core loop works. You can talk to Claude or GPT, read/write files, run bash, and get streaming output. Sessions persist. Compaction works. The main pain points for daily use:

1. **Extension system not wired** — hooks exist but agent loop doesn't fire events to them
2. **Editor is basic** — no @file, no autocomplete, no paste handling
3. **Only 2 providers really work** — Anthropic and OpenAI-compatible
4. **No tree navigation** — /tree is pi's killer feature for branching
5. **TUI components built but not fully integrated** — markdown renderer exists but streaming output uses print!()

### For a "Rust-native extensible agent": **~35% there**

The word "extensible" is key. BB-Agent has the architecture for extensions (event bus, plugin host, JSON-RPC protocol) but:

- **Zero events fire from the agent loop** — `session_start`, `tool_call`, `context`, etc. are defined but never emitted
- **No tool registration from plugins** — `registerTool()` is not implemented
- **No command registration** — `registerCommand()` is not implemented
- **No UI primitives for plugins** — `ctx.ui.*` is not implemented
- **No extension state** — `appendEntry()` is not implemented
- **Plugin host spawns Node but doesn't load plugins** — discovery works, loading doesn't

This means a TypeScript plugin author cannot currently extend BB-Agent in any way.

### For full pi parity: **~18% there (by code)**

Pi has 80K lines of functional code. BB-Agent has 11K. The ratio is roughly right, given:
- Rust is sometimes more verbose, sometimes more concise than TypeScript
- BB-Agent skips many features intentionally (themes, packages, skills, OAuth, etc.)
- But even the implemented features are typically at 25-50% depth vs pi

---

## What to build next (ranked by impact)

### Tier A: Make extensions actually work (biggest differentiator)

| Task | Effort | Impact |
|------|--------|--------|
| Fire hook events from agent loop (session_start, tool_call, tool_result, context, etc.) | 2 days | Critical — without this, "extensible" is marketing |
| Implement registerTool() in plugin host | 2 days | High — custom tools are the main extension use case |
| Actually load and execute TS plugins from disk | 2 days | High — discovery exists but loading doesn't |
| Implement ctx.sessionManager read access for plugins | 1 day | Medium |
| Implement registerCommand() | 1 day | Medium |

### Tier B: Fill provider gaps

| Task | Effort | Impact |
|------|--------|--------|
| Anthropic thinking/reasoning (budget + adaptive) | 1 day | High — current thinking doesn't work properly |
| OpenAI provider quirks (reasoning_effort, developer role) | 1 day | Medium |
| Google Generative AI provider | 2 days | Medium — adds a major provider |
| Context handoff between providers | 2 days | Medium — needed for multi-model workflows |
| Retry with backoff | 1 day | High — reliability |
| Full model registry from generated data | 1 day | Medium |

### Tier C: TUI polish

| Task | Effort | Impact |
|------|--------|--------|
| Tree selector (/tree) | 3 days | High — pi's killer branching feature |
| Overlay system for selectors | 2 days | Medium |
| Editor: autocomplete, @file, paste | 3 days | Medium |
| Markdown rendering in streaming output | 1 day | Medium |
| Loader/spinner during tool execution | 0.5 day | Low |

### Tier D: Session features

| Task | Effort | Impact |
|------|--------|--------|
| Fork session to new file | 1 day | Medium |
| Branch summarization wired to /tree | 1 day | Medium |
| Error recovery (context overflow → compact → retry) | 2 days | High |
| Message queuing (steer + followUp) | 2 days | Medium |

---

## Summary

| Dimension | Status | What's needed |
|-----------|--------|---------------|
| **Can it run a coding session?** | Yes | Works with Anthropic/OpenAI |
| **Is it extensible?** | No (architecture exists, nothing wired) | Wire hooks + implement plugin loading |
| **Does the TUI work?** | Partially (basic display, no overlays) | Integrate markdown, add overlays, improve editor |
| **Is it production-ready?** | No | Needs retry, error recovery, more providers |
| **Is it better than pi in any way?** | Yes: SQLite sessions, artifacts, Rust perf | These are real advantages |
| **How much work to daily-driver?** | ~2 weeks focused | Tier A + key Tier B items |
| **How much work to full extensibility?** | ~4 weeks focused | Tier A fully + plugin lifecycle |
| **How much work to pi parity?** | ~3 months | All tiers + missing features |
