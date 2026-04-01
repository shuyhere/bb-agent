# Pi → BB-Agent 1:1 Port Map

> Every file in pi-mono that needs a Rust equivalent in BB-Agent.
> No invention. Direct port. Copy the logic, translate the syntax.

## Source: /home/shuyhere/tmp/pi-mono

---

## pi-tui → crates/tui/

| Pi file | Lines | → BB file | Status |
|---------|-------|-----------|--------|
| `packages/tui/src/utils.ts` | 1068 | `tui/src/utils.rs` | Rewrite (current is 265 lines, needs full ANSI tracker, wrapTextWithAnsi, extractSegments, sliceByColumn) |
| `packages/tui/src/terminal.ts` | 360 | `tui/src/terminal.rs` | Rewrite (need Kitty protocol, bracketed paste, stdin buffer, drain) |
| `packages/tui/src/tui.ts` | 1200 | `tui/src/tui.rs` | Rewrite (Component trait, Container, TUI class with doRender, overlay system, focus) |
| `packages/tui/src/keys.ts` | 1356 | Skip — use crossterm's key parsing |
| `packages/tui/src/stdin-buffer.ts` | 386 | Skip — crossterm handles this |
| `packages/tui/src/components/editor.ts` | 2230 | `tui/src/components/editor.rs` | Port (bordered editor, cursor, selection, history, autocomplete hook, paste) |
| `packages/tui/src/components/markdown.ts` | 824 | `tui/src/components/markdown.rs` | Improve (current 758 lines, add streaming mode, fix code blocks) |
| `packages/tui/src/components/select-list.ts` | 229 | `tui/src/components/select_list.rs` | Keep (current 383 lines, already works) |
| `packages/tui/src/components/text.ts` | 106 | `tui/src/components/text.rs` | Port |
| `packages/tui/src/components/spacer.ts` | 28 | `tui/src/components/spacer.rs` | Port |
| `packages/tui/src/components/box.ts` | 137 | `tui/src/components/box_component.rs` | Port |
| `packages/tui/src/components/loader.ts` | 55 | `tui/src/components/loader.rs` | Port |
| `packages/tui/src/components/input.ts` | 503 | Skip for now |
| `packages/tui/src/components/settings-list.ts` | 250 | Skip for now |
| `packages/tui/src/components/image.ts` | 104 | Skip for now |
| `packages/tui/src/components/truncated-text.ts` | 65 | `tui/src/components/truncated_text.rs` | Port |
| `packages/tui/src/components/cancellable-loader.ts` | 40 | `tui/src/components/loader.rs` | Merge with loader |
| `packages/tui/src/autocomplete.ts` | 773 | `tui/src/autocomplete.rs` | Port |
| `packages/tui/src/fuzzy.ts` | 133 | `tui/src/fuzzy.rs` | Port |
| `packages/tui/src/keybindings.ts` | 244 | `tui/src/keybindings.rs` | Port |
| `packages/tui/src/editor-component.ts` | 74 | Merge into editor.rs |
| `packages/tui/src/kill-ring.ts` | 46 | `tui/src/kill_ring.rs` | Port |
| `packages/tui/src/undo-stack.ts` | 28 | `tui/src/undo_stack.rs` | Port |

## pi-agent-core → crates/core/ (agent parts)

| Pi file | Lines | → BB file | Status |
|---------|-------|-----------|--------|
| `packages/agent/src/types.ts` | 341 | `core/src/types.rs` | Already done (368 lines) |
| `packages/agent/src/agent-loop.ts` | 631 | `core/src/agent_loop.rs` | Rewrite to match pi exactly |
| `packages/agent/src/agent.ts` | 539 | `core/src/agent.rs` | Port (Agent class with state, events, message queuing) |

## pi-coding-agent/core → crates/core/ + crates/session/

| Pi file | Lines | → BB file | Status |
|---------|-------|-----------|--------|
| `core/agent-session.ts` | 3059 | `core/src/agent_session.rs` | Port (THE central file) |
| `core/session-manager.ts` | 1419 | `session/src/store.rs` + `tree.rs` + `context.rs` | Already done (SQLite version) |
| `core/settings-manager.ts` | 958 | `core/src/settings.rs` | Partially done (377 lines, needs completion) |
| `core/model-registry.ts` | 788 | `provider/src/registry.rs` | Partially done (needs custom model loading) |
| `core/model-resolver.ts` | 628 | `provider/src/resolver.rs` | Done (294 lines) |
| `core/auth-storage.ts` | 493 | `cli/src/login.rs` | Done (242 lines) |
| `core/system-prompt.ts` | 161 | `core/src/system_prompt.rs` | Port |
| `core/bash-executor.ts` | 258 | `tools/src/bash.rs` | Already done |
| `core/compaction/compaction.ts` | 823 | `session/src/compaction.rs` | Partially done |
| `core/compaction/branch-summarization.ts` | 355 | `session/src/compaction.rs` | Partially done |
| `core/compaction/utils.ts` | 197 | `session/src/compaction.rs` | Partially done |
| `core/extensions/types.ts` | 1453 | `hooks/src/events.rs` | Partially done (118 lines, needs full type coverage) |
| `core/extensions/runner.ts` | 915 | `hooks/src/bus.rs` | Partially done (175 lines) |
| `core/extensions/loader.ts` | 557 | `plugin-host/src/host.rs` | Partially done |
| `core/tools/bash.ts` | 431 | `tools/src/bash.rs` | Done |
| `core/tools/edit.ts` | 307 | `tools/src/edit.rs` | Done |
| `core/tools/read.ts` | 208 | `tools/src/read.rs` | Done |
| `core/tools/write.ts` | 131 | `tools/src/write.rs` | Done |
| `core/tools/edit-diff.ts` | 445 | `tools/src/diff.rs` | Partially done |
| `core/tools/find.ts` | 314 | Not yet | Port later |
| `core/tools/grep.ts` | 375 | Not yet | Port later |
| `core/tools/ls.ts` | 160 | Not yet | Port later |
| `core/tools/truncate.ts` | 106 | `tools/src/artifacts.rs` | Done (different approach) |
| `core/slash-commands.ts` | 52 | `cli/src/slash.rs` | Done |
| `core/messages.ts` | 133 | Part of `core/src/types.rs` | Done |
| `core/footer-data-provider.ts` | 339 | `tui/src/footer.rs` | Partially done |
| `core/skills.ts` | 508 | Not yet | Port later |
| `core/prompt-templates.ts` | 256 | Not yet | Port later |
| `core/package-manager.ts` | 2193 | Not yet | Port later |

## pi-coding-agent/modes → crates/cli/

| Pi file | Lines | → BB file | Status |
|---------|-------|-----------|--------|
| `modes/interactive/interactive-mode.ts` | 4624 | `cli/src/interactive/mod.rs` | Rewrite — this is the big one |
| `modes/interactive/theme/theme.ts` | 1133 | `tui/src/theme.rs` | Port later |
| `modes/interactive/components/assistant-message.ts` | 130 | `cli/src/interactive/components/assistant_message.rs` | Port |
| `modes/interactive/components/user-message.ts` | 62 | `cli/src/interactive/components/user_message.rs` | Port |
| `modes/interactive/components/tool-execution.ts` | 328 | `cli/src/interactive/components/tool_execution.rs` | Port |
| `modes/interactive/components/bash-execution.ts` | 218 | `cli/src/interactive/components/bash_execution.rs` | Port |
| `modes/interactive/components/diff.ts` | 147 | `cli/src/interactive/components/diff.rs` | Port |
| `modes/interactive/components/footer.ts` | 220 | `tui/src/footer.rs` | Port |
| `modes/interactive/components/model-selector.ts` | 337 | `cli/src/interactive/components/model_selector.rs` | Port |
| `modes/interactive/components/session-selector.ts` | 1010 | `cli/src/interactive/components/session_selector.rs` | Port |
| `modes/interactive/components/tree-selector.ts` | 1239 | `cli/src/interactive/components/tree_selector.rs` | Port |
| `modes/interactive/components/compaction-summary-message.ts` | 31 | `cli/src/interactive/components/compaction_message.rs` | Port |
| `modes/interactive/components/branch-summary-message.ts` | 25 | `cli/src/interactive/components/branch_summary.rs` | Port |
| `modes/interactive/components/dynamic-border.ts` | 21 | `tui/src/components/border.rs` | Port |
| `modes/interactive/components/bordered-loader.ts` | 30 | `tui/src/components/loader.rs` | Merge |
| `modes/interactive/components/keybinding-hints.ts` | 50 | `cli/src/interactive/components/header.rs` | Merge |
| `modes/print-mode.ts` | 219 | `cli/src/print_mode.rs` | Port |
| `modes/rpc/rpc-mode.ts` | 674 | Not yet | Port later |

## pi-ai → crates/provider/

| Pi file | Lines | → BB file | Status |
|---------|-------|-----------|--------|
| `providers/anthropic.ts` | 905 | `provider/src/anthropic.rs` | Partially done (280 lines) |
| `providers/openai-completions.ts` | 871 | `provider/src/openai.rs` | Partially done (209 lines) |
| `providers/google.ts` | 476 | `provider/src/google.rs` | Partially done |
| `types.ts` | 339 | Part of `core/src/types.rs` + `provider/src/lib.rs` | Partially done |
| `stream.ts` | 59 | `provider/src/streaming.rs` | Done |
| `models.generated.ts` | 14002 | `provider/src/registry.rs` | Need to generate |
| `providers/transform-messages.ts` | 201 | Part of each provider | Not yet |

---

## Next session instruction

Start a new pi session with this prompt:

```
I need to port pi-mono to Rust as BB-Agent at ~/BB-Agent.
Pi source is at /home/shuyhere/tmp/pi-mono.
Read ~/BB-Agent/PORT-MAP.md for the exact file-to-file mapping.

Do NOT invent new architectures. Read each pi TypeScript file,
understand exactly what it does, and translate it to Rust.

Start with Wave 1 foundations using parallel sub-agents via tmux worktrees.
Use `pi -p --no-session @TASK.md` to spawn each sub-agent.

For each sub-agent task, the TASK.md should say:
"Read /home/shuyhere/tmp/pi-mono/packages/X/src/Y.ts line by line.
 Port it to Rust at ~/BB-Agent/crates/Z/src/W.rs.
 Keep the same logic, same behavior, same API surface.
 Translate TypeScript idioms to Rust idioms but don't change the design."
```
