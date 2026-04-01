# BB-Agent Audit: Feature-by-Feature vs Pi

> Line-by-line comparison against
> [pi-mono/packages/coding-agent README](https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent#quick-start)
>
> BB-Agent: 56 files, 10,419 lines of Rust
> Pi coding-agent: 117 files, 41,404 lines of TypeScript

---

## Quick Start

| Pi feature | Pi status | BB status | What's missing |
|------------|-----------|-----------|----------------|
| `npm install -g` | ✓ | ✓ `cargo install` | Works, binary is `bb` |
| `export ANTHROPIC_API_KEY=...` | ✓ | ✓ | Auth via env vars works |
| `pi` (starts interactive) | ✓ | △ | Interactive mode exists but TUI components not wired to agent loop |
| `/login` | ✓ | ✓ | `bb login` works |
| 4 tools: read, write, edit, bash | ✓ | ✓ | All 4 implemented |

**Verdict: △ — agent starts but interactive mode is not properly wired to TUI yet**

---

## Providers & Models

| Pi feature | BB status | Detail |
|------------|-----------|--------|
| Anthropic provider | ✓ | Native Messages API with streaming |
| OpenAI provider | ✓ | Completions API with streaming |
| Google provider | ✗ | Not implemented (Google Generative AI API) |
| Azure OpenAI | ✗ | Not implemented |
| Groq/Cerebras/xAI | △ | OpenAI-compatible, model registered but no quirks handling |
| OpenRouter | △ | Model registered, works via OpenAI-compat |
| Subscriptions (OAuth) | ✗ | `bb login` does API keys only, no OAuth flow |
| Custom providers via models.json | △ | Settings support exists but not fully tested |
| `/model` selector | △ | ModelSelector component built, not wired to interactive mode |
| `--model <pattern>` fuzzy | ✓ | Fuzzy resolver implemented in S3 |
| `--model provider/id:thinking` | ✓ | Parser implemented |
| `--list-models [search]` | ✓ | Works |
| `--models` for Ctrl+P cycling | ✗ | Flag parsed but cycling not implemented |
| Mid-session model switching | ✗ | No model change entry written to session |
| Context handoff between providers | ✗ | No thinking trace conversion |

**Verdict: △ — Anthropic + OpenAI work, but many providers and features missing**

---

## Interactive Mode

### UI Structure

| Pi feature | BB status | Detail |
|------------|-----------|--------|
| Startup header | ✗ | Not implemented |
| Messages display | △ | Chat rendering exists but not via differential renderer |
| Streaming token display | △ | Events stream but display is inline print, not TUI component |
| Markdown rendering | ✓ (built) | `markdown.rs` (769 lines) built but not wired |
| Tool call display | △ | Shows tool name + brief result, no expand/collapse |
| Editor with prompt | △ | Editor component (508 lines) built but not wired to interactive |
| Footer (model, tokens, cost) | △ | Status component built but not wired |
| Differential rendering | ✓ (built) | `renderer.rs` built but not used in interactive loop |
| Synchronized output (flicker-free) | ✓ (built) | Terminal component has sync output support |

### Editor Features

| Pi feature | BB status | Detail |
|------------|-----------|--------|
| Multi-line editing | ✓ (built) | Editor supports multi-line |
| `@` file reference (fuzzy search) | ✗ | Not implemented |
| Tab path completion | ✗ | Not implemented |
| Shift+Enter for newline | ✓ (built) | Editor supports |
| Ctrl+V image paste | ✗ | Not implemented |
| Drag & drop images | ✗ | Not implemented |
| `!command` bash | ✓ | Works in interactive loop |
| `!!command` bash (no context) | ✓ | Works |
| Undo/redo | ✗ | Not in current editor |
| Kill ring (Ctrl+K/Y) | ✗ | Not in current editor |
| Word jump (Ctrl+Left/Right) | ✓ (built) | Editor supports |
| History (Up/Down) | ✓ (built) | Editor supports |

### Commands

| Command | Pi | BB | Detail |
|---------|-----|-----|--------|
| `/login` | ✓ | ✓ | Works |
| `/logout` | ✓ | ✓ | Works |
| `/model` | ✓ | △ | Slash handler exists, selector component built, not wired |
| `/scoped-models` | ✓ | ✗ | Not implemented |
| `/settings` | ✓ | ✗ | Not implemented (shows path only) |
| `/resume` | ✓ | △ | Lists sessions in text, selector component built, not wired |
| `/new` | ✓ | △ | Prints message, doesn't actually create new session |
| `/name <name>` | ✓ | △ | Prints message, doesn't persist |
| `/session` | ✓ | ✗ | Not implemented |
| `/tree` | ✓ | ✗ | Not implemented (tree selector not built) |
| `/fork` | ✓ | ✗ | Not implemented |
| `/compact` | ✓ | △ | Handler exists, compaction logic built (S2), not wired |
| `/copy` | ✓ | ✗ | Not implemented |
| `/export` | ✓ | ✗ | Not implemented |
| `/share` | ✓ | ✗ | Not implemented |
| `/reload` | ✓ | ✗ | Not implemented |
| `/hotkeys` | ✓ | ✗ | Not implemented |
| `/changelog` | ✓ | ✗ | Not implemented |
| `/quit` | ✓ | ✓ | Works |
| `/help` | ✓ | ✓ | Works |

### Keyboard Shortcuts

| Shortcut | Pi | BB | Detail |
|----------|-----|-----|--------|
| Ctrl+C clear/abort | ✓ | △ | Clears editor, no abort of running agent |
| Ctrl+C twice quit | ✓ | ✗ | Not implemented |
| Escape abort | ✓ | ✗ | Not implemented |
| Escape twice → /tree | ✓ | ✗ | Not implemented |
| Ctrl+L model selector | ✓ | ✗ | Not implemented |
| Ctrl+P model cycle | ✓ | ✗ | Not implemented |
| Shift+Tab thinking cycle | ✓ | ✗ | Not implemented |
| Ctrl+O expand/collapse tools | ✓ | ✗ | Not implemented |
| Ctrl+T expand/collapse thinking | ✓ | ✗ | Not implemented |

### Message Queue

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| Enter queues steering msg | ✓ | ✗ | Not implemented |
| Alt+Enter queues follow-up | ✓ | ✗ | Not implemented |
| Escape restores queued | ✓ | ✗ | Not implemented |
| Alt+Up retrieves queued | ✓ | ✗ | Not implemented |

---

## Sessions

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| Auto-save to disk | ✓ | ✓ | SQLite (better than pi's JSONL for scale) |
| `bb -c` continue | ✓ | ✓ | Works |
| `bb -r` resume picker | ✓ | △ | Lists sessions, no interactive picker wired |
| `--no-session` ephemeral | ✓ | ✓ | Flag exists |
| `--session <path>` | ✓ | △ | Flag parsed but not implemented |
| `--fork <path>` | ✓ | ✗ | Not implemented |
| `--session-dir` | ✓ | ✗ | Not implemented |
| Session tree structure | ✓ | ✓ | `id`/`parentId` tree, tested |
| `/tree` navigation | ✓ | ✗ | Tree selector not built |
| `/fork` create branch session | ✓ | ✗ | Not implemented |
| Branch summarization | ✓ | △ | Logic partially in compaction.rs, not wired |
| Session naming | ✓ | △ | Slash handler exists, not persisted |

### Compaction

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| Auto-compact on overflow | ✓ | △ | `should_compact()` exists, not triggered in agent loop |
| `/compact` manual | ✓ | △ | Handler exists, `compact()` fn built (S2), not wired |
| Structured summary format | ✓ | ✓ | Summarization prompt implemented (S2) |
| Split-turn compaction | ✓ | △ | `is_split_turn` flag exists, dual-summary not implemented |
| Iterative compaction | ✓ | △ | `previous_summary` tracking exists, not tested end-to-end |
| File operation tracking | ✓ | ✓ | `extract_file_operations()` implemented (S2) |
| Conversation serialization | ✓ | ✓ | `serialize_conversation()` implemented (S2) |
| Extension override | ✓ | ✗ | Hook exists but not wired |

---

## Settings

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| `~/.bb-agent/settings.json` global | ✓ | ✓ | Implemented (S3) |
| `.bb-agent/settings.json` project | ✓ | ✓ | Implemented (S3) |
| Merge (project overrides global) | ✓ | ✓ | Implemented (S3) |
| `/settings` UI | ✓ | ✗ | Not implemented |
| Compaction settings | ✓ | ✓ | In settings struct |
| Default model/provider | ✓ | ✓ | In settings struct |
| Custom models in settings | ✓ | ✓ | `ModelOverride` in settings (S3) |
| Custom providers in settings | ✓ | △ | `ProviderOverride` struct exists, not applied |

---

## Context Files

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| AGENTS.md loading | ✓ | △ | Loads from cwd and global, no parent dir scanning |
| Parent directory scanning | ✓ | ✗ | Not implemented |
| CLAUDE.md alias | ✓ | ✗ | Not implemented |
| `.bb-agent/SYSTEM.md` replacement | ✓ | ✗ | Not implemented |
| `APPEND_SYSTEM.md` | ✓ | ✗ | Not implemented |
| `--system-prompt` override | ✓ | ✓ | Works |
| `--append-system-prompt` | ✓ | ✓ | Works |

---

## Customization

### Prompt Templates

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| Markdown template files | ✓ | ✗ | Not implemented |
| `/template-name` invocation | ✓ | ✗ | Not implemented |
| Argument substitution ($@, $1) | ✓ | ✗ | Not implemented |
| Frontmatter (description) | ✓ | ✗ | Not implemented |

### Skills

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| SKILL.md loading | ✓ | ✗ | Not implemented |
| `/skill:name` invocation | ✓ | ✗ | Not implemented |
| Auto-discovery | ✓ | ✗ | Not implemented |
| System prompt formatting | ✓ | ✗ | Not implemented |

### Extensions

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| TypeScript extension loading | ✓ | △ | Plugin host exists, discovery works, not wired to lifecycle |
| `pi.on("event", handler)` | ✓ | △ | Event bus exists, not connected to agent session |
| `pi.registerTool()` | ✓ | ✗ | Not implemented |
| `pi.registerCommand()` | ✓ | ✗ | Not implemented |
| `pi.registerShortcut()` | ✓ | ✗ | Not implemented |
| `pi.sendMessage()` | ✓ | ✗ | Not implemented |
| `pi.appendEntry()` | ✓ | ✗ | Not implemented |
| `ctx.ui.*` dialogs | ✓ | ✗ | Not implemented |
| Custom tool rendering | ✓ | ✗ | Not implemented |
| Hot reload (`/reload`) | ✓ | ✗ | Not implemented |
| `-e <path>` load extension | ✓ | △ | Flag parsed but not implemented |

### Themes

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| JSON theme files | ✓ | ✗ | Not implemented |
| Theme hot-reload | ✓ | ✗ | Not implemented |
| `/settings` theme selection | ✓ | ✗ | Not implemented |
| Built-in dark/light | ✓ | ✗ | Hardcoded colors only |

### Pi Packages

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| `pi install npm:...` | ✓ | ✗ | Not implemented |
| `pi install git:...` | ✓ | ✗ | Not implemented |
| `pi remove` | ✓ | ✗ | Not implemented |
| `pi update` | ✓ | ✗ | Not implemented |
| `pi list` | ✓ | ✗ | Not implemented |
| `pi config` | ✓ | ✗ | Not implemented |

---

## Programmatic Usage

| Feature | Pi | BB | Detail |
|---------|-----|-----|--------|
| SDK (`createAgentSession`) | ✓ | △ | AgentSession exists (S1), no public SDK API |
| RPC mode (`--mode rpc`) | ✓ | ✗ | Not implemented |
| JSON mode (`--mode json`) | ✓ | ✗ | Not implemented |
| Print mode (`-p`) | ✓ | ✓ | Works |
| Piped stdin | ✓ | ✗ | Not implemented |

---

## CLI Reference

### Flags

| Flag | Pi | BB | Detail |
|------|-----|-----|--------|
| `--provider` | ✓ | ✓ | Works |
| `--model` | ✓ | ✓ | With fuzzy matching |
| `--api-key` | ✓ | ✓ | Works |
| `--system-prompt` | ✓ | ✓ | Works |
| `--append-system-prompt` | ✓ | ✓ | Works |
| `--thinking` | ✓ | △ | Parsed but not applied to provider request |
| `-p, --print` | ✓ | ✓ | Works |
| `-c, --continue` | ✓ | ✓ | Works |
| `-r, --resume` | ✓ | △ | Lists sessions, no interactive picker |
| `--session` | ✓ | △ | Parsed, not implemented |
| `--fork` | ✓ | ✗ | Not implemented |
| `--session-dir` | ✓ | ✗ | Not implemented |
| `--no-session` | ✓ | ✓ | Works |
| `--tools` | ✓ | △ | Parsed, not applied |
| `--no-tools` | ✓ | △ | Parsed, not applied |
| `--models` | ✓ | △ | Parsed, cycling not implemented |
| `--list-models` | ✓ | ✓ | Works |
| `--export` | ✓ | ✗ | Not implemented |
| `--mode json/rpc` | ✓ | ✗ | Not implemented |
| `--verbose` | ✓ | ✓ | Works |
| `-e, --extension` | ✓ | △ | Parsed, not loaded |
| `--no-extensions` | ✓ | ✗ | Not implemented |
| `--skill` | ✓ | ✗ | Not implemented |
| `--no-skills` | ✓ | ✗ | Not implemented |
| `--prompt-template` | ✓ | ✗ | Not implemented |
| `--theme` | ✓ | ✗ | Not implemented |
| `@files` | ✓ | ✗ | Not implemented |

---

## Summary scoreboard

| Category | Total features | ✓ Done | △ Partial | ✗ Missing |
|----------|---------------|--------|-----------|-----------|
| **Quick Start** | 5 | 4 | 1 | 0 |
| **Providers** | 14 | 4 | 4 | 6 |
| **Interactive UI** | 10 | 0 | 6 | 4 |
| **Editor** | 12 | 5 | 0 | 7 |
| **Commands** | 17 | 3 | 5 | 9 |
| **Keyboard shortcuts** | 9 | 0 | 1 | 8 |
| **Message queue** | 4 | 0 | 0 | 4 |
| **Sessions** | 12 | 5 | 3 | 4 |
| **Compaction** | 8 | 3 | 4 | 1 |
| **Settings** | 8 | 5 | 1 | 2 |
| **Context files** | 6 | 2 | 1 | 3 |
| **Prompt templates** | 4 | 0 | 0 | 4 |
| **Skills** | 4 | 0 | 0 | 4 |
| **Extensions** | 11 | 0 | 3 | 8 |
| **Themes** | 4 | 0 | 0 | 4 |
| **Packages** | 6 | 0 | 0 | 6 |
| **Programmatic** | 5 | 1 | 1 | 3 |
| **CLI flags** | 25 | 11 | 8 | 6 |
| **TOTAL** | **164** | **43 (26%)** | **38 (23%)** | **83 (51%)** |

---

## Critical path to "usable like pi"

These are the minimum features needed for BB-Agent to be usable as a daily coding agent:

### Tier 1: Must have (makes bb actually work)

| # | What | Why | Est. effort |
|---|------|-----|-------------|
| 1 | **Wire TUI into agent loop** | Without this, all TUI components are dead code | 2 days |
| 2 | **Wire compaction into agent loop** | Sessions will crash without auto-compact | 1 day |
| 3 | **Apply --thinking to provider** | Thinking/reasoning doesn't work yet | 0.5 day |
| 4 | **Apply --tools filtering** | Can't restrict tools | 0.5 day |
| 5 | **Abort on Escape/Ctrl+C** | Can't cancel a running agent | 1 day |
| 6 | **Streaming bash display** | Bash waits for finish, no feedback | 1 day |
| 7 | **Write model_change entry on switch** | Session doesn't track model changes | 0.5 day |

### Tier 2: Should have (expected coding agent UX)

| # | What | Why | Est. effort |
|---|------|-----|-------------|
| 8 | Edit diff display | Expected to see what changed | 1 day |
| 9 | `/model` wired to selector | Need to switch models interactively | 0.5 day |
| 10 | `/resume` wired to selector | Need to resume sessions | 0.5 day |
| 11 | `/compact` wired to compaction | Need manual compaction | 0.5 day |
| 12 | `/new` creates new session | Need to start fresh | 0.5 day |
| 13 | `/name` persists to session | Need session naming | 0.5 day |
| 14 | Parent dir AGENTS.md scanning | Need hierarchical context | 0.5 day |
| 15 | `--session <id>` resolve + open | Need to open specific sessions | 0.5 day |
| 16 | `@file` arguments | Need to include files in prompt | 1 day |
| 17 | Piped stdin | `cat file | bb -p "summarize"` | 0.5 day |

### Tier 3: Nice to have (polish)

| # | What | Est. effort |
|---|------|-------------|
| 18 | Google Generative AI provider | 2 days |
| 19 | `/tree` navigation | 2 days |
| 20 | `/fork` | 1 day |
| 21 | Ctrl+P model cycling | 1 day |
| 22 | Shift+Tab thinking cycling | 0.5 day |
| 23 | Ctrl+O tool expand/collapse | 0.5 day |
| 24 | Message queue (steer/followUp) | 2 days |
| 25 | Context handoff between providers | 1 day |
| 26 | Ctrl+C twice to quit | 0.5 day |
| 27 | `/session` info display | 0.5 day |
| 28 | `/hotkeys` display | 0.5 day |
| 29 | Extension lifecycle wiring | 2 days |
| 30 | `@` file fuzzy search in editor | 2 days |

### Tier 4: Defer (not needed for daily use)

| What | Why defer |
|------|----------|
| Skills | Use AGENTS.md instead |
| Prompt templates | Use slash commands manually |
| Themes | Hardcoded colors work |
| Packages | Manual install works |
| HTML export | Not critical |
| RPC/JSON mode | Not needed until IDE integration |
| OAuth subscriptions | API keys work |
| `/share` | Not critical |
| `/copy` clipboard | Not critical |
| `/changelog` | Not critical |
| Image paste/drag | Not critical |
| Undo/redo in editor | Not critical |
| Kill ring | Not critical |

---

## Summary

**BB-Agent has 26% of pi's features fully working, 23% partially built, and 51% missing.**

The biggest blocker is **Tier 1 item #1: wire TUI into agent loop**. The TUI components
(editor, markdown, select list, model selector, session selector, status bar, differential
renderer) are all built but sitting as dead code. The interactive mode controller (`interactive.rs`)
was built in S4 but needs to be properly integrated with the `AgentSession` from S1.

**Estimated effort to reach "usable daily":**
- Tier 1 (must have): ~6.5 days
- Tier 2 (should have): ~6 days
- Total for daily use: **~2.5 weeks**

**To reach full pi parity** (all tiers including defer):
- Additional ~4 weeks for Tier 3 + Tier 4
- Total: **~6.5 weeks from today**
