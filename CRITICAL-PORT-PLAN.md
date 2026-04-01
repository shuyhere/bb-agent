# BB-Agent Critical Port Plan

Goal: finish the highest-leverage remaining ports from `pi-mono` into Rust so BB-Agent stops being a partial component port and becomes a true pi-style runtime.

This plan focuses only on the critical remaining gaps.

---

## Current reality

Already merged into `master`:
- shared TUI primitives/components: text, spacer, truncated_text, box_component, border, loader
- footer data/provider pieces
- fuzzy / kill_ring / undo_stack
- interactive display component files: assistant, header, tool, bash, user, compaction, branch, diff

Still missing structurally:
- a real Rust equivalent of `interactive-mode.ts`
- a real Rust equivalent of `agent-session.ts`
- a real Rust equivalent of `agent.ts`
- a real Rust equivalent of `agent-loop.ts`
- proper wiring of new components into the running interactive path
- autocomplete, keybindings, overlays, selectors, queue behavior, and full pi interaction semantics

---

# Priority order

## P0 — The central runtime ports
These are the highest leverage. Until they are ported, BB-Agent will not feel like pi.

1. `packages/coding-agent/src/modes/interactive/interactive-mode.ts`
2. `packages/coding-agent/src/core/agent-session.ts`
3. `packages/agent/src/agent.ts`
4. `packages/agent/src/agent-loop.ts`

## P1 — TUI engine parity
5. `packages/tui/src/tui.ts`
6. `packages/tui/src/terminal.ts`
7. `packages/tui/src/components/editor.ts`
8. `packages/tui/src/autocomplete.ts`
9. `packages/tui/src/keybindings.ts`

## P2 — Interactive UX completeness
10. model selector
11. session selector
12. tree selector
13. settings/login/dialog overlays
14. message queue / follow-up semantics
15. thinking/tool expand-collapse interaction

## P3 — Coding-agent completeness
16. find / grep / ls tools
17. model registry depth
18. extensions / themes / skills / packages
19. print/json/rpc parity details
20. footer + status + context compaction parity details

---

# Exact remaining pi → BB file mapping

## Phase A — Port AgentSession runtime first

### Pi source
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/core/agent-session.ts`

### BB targets
- `crates/core/src/agent_session.rs` (new)
- `crates/core/src/system_prompt.rs` (new or expanded)
- `crates/core/src/lib.rs`
- possibly small interface additions in:
  - `crates/session/src/context.rs`
  - `crates/session/src/store.rs`
  - `crates/provider/src/lib.rs`
  - `crates/hooks/src/lib.rs`

### Why first
This file is pi’s actual center of gravity. It owns:
- prompt submission
- model binding
- compaction triggering/recovery
- event subscription
- session navigation hooks
- extension binding points
- runtime state

### Acceptance criteria
- `interactive.rs` no longer manually owns most session/runtime logic
- one Rust `AgentSession` owns prompt execution and state mutation
- compaction trigger lives here
- model switching and thinking switching live here
- streaming events come out of this layer

### Current BB gap
Right now BB still spreads this across CLI + session + provider pieces.

---

## Phase B — Port generic agent core

### Pi sources
- `/home/shuyhere/tmp/pi-mono/packages/agent/src/agent.ts`
- `/home/shuyhere/tmp/pi-mono/packages/agent/src/agent-loop.ts`
- `/home/shuyhere/tmp/pi-mono/packages/agent/src/types.ts`

### BB targets
- `crates/core/src/agent.rs`
- `crates/core/src/agent_loop.rs`
- `crates/core/src/types.rs`
- `crates/core/src/lib.rs`

### Why second
Without this, BB still has the real loop in `crates/cli/src/agent_loop.rs`, which is architecturally wrong for a pi-style reconstruction.

### Acceptance criteria
- `crates/cli/src/agent_loop.rs` becomes thin wrapper or is deleted
- generic loop lives in `bb-core`
- loop returns/streams event types similar to pi
- tool-call / assistant-turn lifecycle is runtime-owned, not CLI-owned

### Current BB gap
- `crates/core/src/agent.rs` is only ~83 lines
- `crates/core/src/agent_loop.rs` is only ~28 lines
- real work still happens in CLI

---

## Phase C — Port interactive-mode.ts architecture

### Pi source
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/modes/interactive/interactive-mode.ts`

### BB targets
- split current:
  - `crates/cli/src/interactive.rs`
  - `crates/cli/src/interactive/mod.rs`
- into something like:
  - `crates/cli/src/interactive/mod.rs`
  - `crates/cli/src/interactive/controller.rs` (new)
  - `crates/cli/src/interactive/state.rs` (new)
  - `crates/cli/src/interactive/commands.rs` (new)
  - `crates/cli/src/interactive/overlays.rs` (new)

### Why third
This is what will actually make `bb` feel like pi.

### Port responsibilities from pi
- startup header construction
- component tree assembly
- event subscription to AgentSession
- tool execution component lifecycle
- slash command routing
- overlays/dialogs/selectors
- queue behavior
- abort behavior
- pending messages area
- focus switching and editor replacement
- footer updates

### Acceptance criteria
- `interactive.rs` no longer directly hand-renders most chat/tool/footer/editor logic
- new interactive components are actually instantiated and used
- interactive mode becomes stateful controller around TUI + AgentSession
- slash/dialog behavior becomes pi-like

### Current BB gap
New component files exist, but are mostly not driving the visible runtime yet.

---

## Phase D — Deepen TUI engine parity

### Pi sources
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/tui.ts`
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/terminal.ts`
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/utils.ts`

### BB targets
- `crates/tui/src/tui_core.rs`
- `crates/tui/src/terminal.rs`
- `crates/tui/src/renderer.rs`
- `crates/tui/src/utils.rs`
- `crates/tui/src/component.rs`

### Why fourth
BB now has the skeleton, but the engine is still much thinner than pi.

### Acceptance criteria
- overlay support exists in TUI engine, not only controller hacks
- focus model matches pi better
- differential rendering matches pi behavior more closely
- terminal handling includes richer raw input / paste / resize behavior
- rendering and cursor logic are robust under editor + overlays

### Current BB gap
`tui_core.rs` is ~123 lines vs pi `tui.ts` ~1200.

---

## Phase E — Finish the editor to pi parity

### Pi sources
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/components/editor.ts`
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/editor-component.ts`
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/kill-ring.ts`
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/undo-stack.ts`

### BB targets
- `crates/tui/src/editor.rs`
- `crates/tui/src/kill_ring.rs`
- `crates/tui/src/undo_stack.rs`

### Why fifth
Editor is the most visible pi interaction surface.

### Acceptance criteria
- bordered editor matches pi behavior
- queue/submit behavior integrates with controller
- history browsing matches interactive semantics
- selection/paste/word operations are robust
- slash and autocomplete hooks are editor-native, not bolted on

### Current BB gap
Editor is far improved but still much smaller than pi’s implementation.

---

## Phase F — Autocomplete + keybindings

### Pi sources
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/autocomplete.ts`
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/keybindings.ts`
- `/home/shuyhere/tmp/pi-mono/packages/tui/src/fuzzy.ts`

### BB targets
- `crates/tui/src/autocomplete.rs` (new)
- `crates/tui/src/keybindings.rs` (new)
- `crates/tui/src/fuzzy.rs` (already partial)
- `crates/tui/src/editor.rs`
- `crates/cli/src/interactive/controller.rs` or equivalent

### Acceptance criteria
- `@file` fuzzy search works in editor
- slash completion works from `/`
- keybinding system exists outside hardcoded match arms
- Ctrl+L / Ctrl+P / Shift+Tab / Ctrl+O / Ctrl+T behave pi-like

---

## Phase G — Selectors and overlays

### Pi sources
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/modes/interactive/components/model-selector.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/modes/interactive/components/session-selector.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/modes/interactive/components/tree-selector.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/modes/interactive/components/login-dialog.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/modes/interactive/components/settings-selector.ts`

### BB targets
- current existing files or new equivalents:
  - `crates/tui/src/model_selector.rs`
  - `crates/tui/src/session_selector.rs`
  - `crates/tui/src/tree_selector.rs`
  - or migrate them under `crates/cli/src/interactive/components/`
- `crates/cli/src/interactive/overlays.rs`

### Acceptance criteria
- model/session/tree selection show as obvious overlays/dialogs
- focus capture/release works
- slash commands route into overlays cleanly

---

## Phase H — Tools parity for coding agent

### Pi sources
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/core/tools/find.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/core/tools/grep.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/core/tools/ls.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/core/tools/file-mutation-queue.ts`

### BB targets
- `crates/tools/src/find.rs` (new)
- `crates/tools/src/grep.rs` (new)
- `crates/tools/src/ls.rs` (new)
- `crates/tools/src/lib.rs`
- maybe a mutation queue helper under `crates/tools/`

### Acceptance criteria
- tool list matches pi defaults more closely
- slash/help/model prompts can truthfully advertise the same tooling surface

---

## Phase I — Provider/runtime parity

### Pi sources
- `/home/shuyhere/tmp/pi-mono/packages/ai/src/providers/*.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/core/model-registry.ts`
- `/home/shuyhere/tmp/pi-mono/packages/coding-agent/src/core/model-resolver.ts`

### BB targets
- `crates/provider/src/*.rs`
- `crates/provider/src/registry.rs`
- `crates/provider/src/resolver.rs`
- maybe generated model data machinery

### Acceptance criteria
- broader model registry parity
- more faithful provider/model selection behavior
- context window / pricing / thinking flags accurate enough for footer and routing

---

## Phase J — Extensions / skills / themes / packages

### Pi sources
- `packages/coding-agent/src/core/extensions/*`
- `packages/coding-agent/src/core/package-manager.ts`
- `packages/coding-agent/src/modes/interactive/theme/*`
- skill/prompt/template resource loaders

### BB targets
- `crates/hooks/*`
- `crates/plugin-host/*`
- likely new theme/resource loader modules

### Acceptance criteria
- BB reaches pi-level extensibility surface
- not critical for first UX parity, but critical for “full reconstruction” claim

---

# Recommended implementation order inside BB-Agent

## Step 1
Create `crates/core/src/agent_session.rs` and move session runtime responsibility there.

## Step 2
Move the real agent loop out of `crates/cli/src/agent_loop.rs` into `crates/core/src/agent.rs` + `crates/core/src/agent_loop.rs`.

## Step 3
Replace current `crates/cli/src/interactive.rs` with a controller port organized around:
- TUI
- AgentSession
- interactive components
- overlays
- commands

## Step 4
Deepen `crates/tui/src/tui_core.rs`, `terminal.rs`, and `renderer.rs` until they can support the controller without hacks.

## Step 5
Wire the already-ported interactive components into live runtime events.

## Step 6
Add autocomplete/keybindings/selectors.

## Step 7
Only after the above, continue with remaining provider/extensions/tools parity.

---

# Concrete “done means done” checklist

A reconstruction is not close to complete until all are true:

- [ ] `bb` no longer uses a mostly custom interactive controller unrelated to pi’s `interactive-mode.ts`
- [ ] new interactive component files are actually used in the running UI
- [ ] agent runtime is centered in `AgentSession`-like Rust type
- [ ] generic loop/session logic is no longer trapped in CLI
- [ ] overlays/selectors are obvious and pi-like
- [ ] editor supports `@file`, slash completion, and queue semantics like pi
- [ ] footer/model/context/thinking behavior match pi closely
- [ ] tools surface includes `find`, `grep`, `ls`
- [ ] tree/session/model dialogs are properly wired
- [ ] terminal UX visually matches pi in real terminal testing

---

# Immediate next coding task

If continuing right now, the highest-value task is:

## Port `agent-session.ts`

Target:
- `crates/core/src/agent_session.rs`

Then immediately after:

## Port `interactive-mode.ts` controller structure

Targets:
- `crates/cli/src/interactive/mod.rs`
- `crates/cli/src/interactive/controller.rs`
- `crates/cli/src/interactive/commands.rs`
- `crates/cli/src/interactive/overlays.rs`

That is the shortest path from “component foundation” to “pi-like runtime behavior”.
