# Fullscreen TUI Polish Plan

## Audit Summary (against Rust Code Principles)

### Violations Found

| Principle | Violation | Severity | Where |
|-----------|-----------|----------|-------|
| **#5 No giant files** | `runtime.rs` = 2,871 lines, `fullscreen_entry.rs` = 1,823 lines | 🔴 Critical | Both files |
| **#5 One module = one responsibility** | `FullscreenState` handles 8 concerns: input, keys, focus, search, menus, formatting, streaming, commands | 🔴 Critical | `runtime.rs` |
| **#5 One module = one responsibility** | `FullscreenController` handles 9 concerns: DB, auth, models, turns, menus, formatting, settings, clipboard, transcript | 🔴 Critical | `fullscreen_entry.rs` |
| **#21 No giant files** | `apply_command` is 178 lines (one match arm) | 🟡 Medium | `runtime.rs:541` |
| **#21 No giant files** | `on_normal_key` is 140 lines | 🟡 Medium | `runtime.rs:720` |
| **#1 Minimize cloning** | 58 `.clone()` calls in `fullscreen_entry.rs` | 🟡 Medium | CLI controller |
| **#4 Strong typing** | `FullscreenState` has 18 `pub` fields out of 24 — struct internals fully exposed | 🟡 Medium | `runtime.rs` |
| **#20 No hidden side effects** | `apply_command` mutates state + returns intent (side effect + value) | 🟢 Low | Acceptable pattern |
| **#2 No expect in production** | 2 `.expect()` in `projection.rs` production code | 🟢 Low | Char boundary |
| **#14 Dependency hygiene** | `format_tool_result_content` duplicated between `runtime.rs` and `fullscreen_entry.rs` | 🟡 Medium | Both files |
| **#10 Avoid unnecessary allocations** | Functions like `mode_help_text()` → `String` called on every tick could be `&'static str` | 🟢 Low | `runtime.rs` |

### What's Already Good

| Principle | Status |
|-----------|--------|
| **#2 No unwrap in production** | ✅ Zero `.unwrap()` in production fullscreen code |
| **#3 No hardcoded secrets** | ✅ Clean |
| **#7 Async best practices** | ✅ tokio runtime, no blocking in async |
| **#8 No println in production** | ✅ Zero `println!`/`eprintln!` |
| **#12 Testing** | ✅ 31 tests for `runtime.rs`, good coverage |
| **#6 Traits for abstraction** | ✅ `LocalSlashCommandHost` trait in controller |
| **#9 Typed config** | ✅ `FullscreenAppConfig` struct |
| **#16 Serde for data** | ✅ Proper serialization |

---

## Refactor Plan (10 Phases)

Each phase: one commit, compiles, all tests pass, no behavior change.

### Phase 1: Extract types → `types.rs` *(~150 lines out)*

**Principle #4, #5: Strong typing, separate types from logic**

Move from `runtime.rs`:
- `FullscreenAppConfig` + `Default` impl
- `FullscreenOutcome`
- `FullscreenFooterData`
- `FullscreenCommand` (17 variants)
- `FullscreenSubmission`
- `FullscreenNoteLevel`
- `FullscreenMode`
- `FullscreenSearchState`

→ **New file:** `crates/tui/src/fullscreen/types.rs`
→ `runtime.rs` imports from `types.rs`
→ Risk: **Low** (pure move, no logic)

### Phase 2: Extract tool formatting → `tool_format.rs` *(~350 lines out, kills duplication)*

**Principle #5, #14: One responsibility, no duplication**

Move from `runtime.rs` all free functions:
- `format_tool_call_title`
- `format_tool_call_content`
- `format_tool_result_content`
- `format_write_call_content`
- `format_edit_call_content`
- `preview_tool_result_lines`
- `tool_result_preview_limit`
- `summarize_inline`
- `shorten_display_path`

→ **New file:** `crates/tui/src/fullscreen/tool_format.rs`
→ Make `pub(crate)` so `fullscreen_entry.rs` can reuse
→ Delete duplicate `format_tool_result_content` from `fullscreen_entry.rs`
→ Risk: **Low** (pure functions, no state)

### Phase 3: Extract input editing → `input.rs` *(~100 lines out)*

**Principle #5: One responsibility**

Move `impl FullscreenState` methods:
- `insert_char`, `insert_str`
- `backspace`, `move_left`, `move_right`
- `submit_input`, `submit_local_command`

Move free functions:
- `previous_boundary`, `next_boundary`

→ **New file:** `crates/tui/src/fullscreen/input.rs`
→ Uses `impl FullscreenState` in a separate file (Rust allows this within same crate)
→ Risk: **Low** (self-contained, no cross-cutting deps)

### Phase 4: Extract menus → `menus.rs` *(~200 lines out)*

**Principle #5: One responsibility**

Move:
- `FullscreenSlashMenuState` + all impl
- `FullscreenSelectMenuState` + all impl
- `colorize_tree_menu_label`
- Methods: `slash_query`, `update_slash_menu`, `accept_slash_selection`
- Methods: `render_select_menu_lines`, `render_slash_menu_lines`

→ **New file:** `crates/tui/src/fullscreen/menus.rs`
→ Risk: **Low** (isolated state)

### Phase 5: Extract focus/navigation → `navigation.rs` *(~180 lines out)*

**Principle #5: One responsibility**

Move `impl FullscreenState` methods:
- `focus_block`, `set_focused_block`
- `focus_first`, `focus_last`
- `move_focus`, `page_move`
- `ensure_focus_visible`
- `sync_focus_tracking`, `focus_row_for_block`
- `focusable_blocks`, `visible_header_blocks`, `default_focus_block`
- `first_focusable_block`, `last_focusable_block`
- `focus_first_visible_block`, `focus_last_visible_block`

→ **New file:** `crates/tui/src/fullscreen/navigation.rs`
→ Risk: **Medium** (methods reference other state fields)

### Phase 6: Extract search → `search.rs` *(~100 lines out)*

**Principle #5: One responsibility**

Move:
- `on_search_key`
- `search_step`, `block_matches_query`
- `search_prompt`

→ **New file:** `crates/tui/src/fullscreen/search.rs`
→ Risk: **Low** (small, focused)

### Phase 7: Extract key/event handlers → `events.rs` *(~300 lines out)*

**Principle #5: One responsibility**

Move:
- `on_key`, `on_normal_key`, `on_transcript_key`
- `on_paste`, `on_mouse`, `on_resize`, `on_tick`
- `toggle_transcript_mode`
- `toggle_block`, `toggle_focused_block`, `expand_focused_block`, `collapse_focused_block`

→ **New file:** `crates/tui/src/fullscreen/events.rs`
→ Risk: **Medium** (routes to navigation, input, search, menus)

### Phase 8: Extract streaming/turn state → `streaming.rs` *(~200 lines out)*

**Principle #5: One responsibility**

Move:
- `ActiveTurnState` + impl
- `ToolCallState`
- `finish_active_turn`, `ensure_active_turn_root`
- `ensure_assistant_content_block`, `ensure_thinking_block`
- `ensure_tool_result_block`
- `tool_call_state`, `tool_call_state_mut`
- `refresh_tool_rendering`

→ **New file:** `crates/tui/src/fullscreen/streaming.rs`
→ Risk: **Medium** (state management, modify FullscreenState)

### Phase 9: Extract tests → `tests.rs` *(~930 lines out)*

**Principle #12: Tests at module boundaries**

Move all `#[cfg(test)] mod tests` from `runtime.rs`.

→ **New file:** `crates/tui/src/fullscreen/tests.rs`
→ Risk: **Low** (pure move)

### Phase 10: Split `fullscreen_entry.rs` → `crates/cli/src/fullscreen/` *(~1,823 lines)*

**Principle #5: Modular design**

Split the god controller into focused modules:

| New File | Content | Est. Lines |
|----------|---------|------------|
| `mod.rs` | Entry point `run_fullscreen`, re-exports | ~50 |
| `controller.rs` | `FullscreenController` struct + `run` + `handle_submission` | ~300 |
| `session.rs` | DB ops, transcript building, session create/switch/fork | ~350 |
| `auth.rs` | Login/logout menu, auth state | ~150 |
| `models.rs` | Model selection/cycling, settings menus | ~300 |
| `turns.rs` | `run_streaming_turn_loop`, `handle_turn_event`, `build_turn_config` | ~300 |
| `formatting.rs` | Shared formatters (reuse `tool_format.rs`), footer data | ~200 |
| `menus.rs` | `handle_menu_selection`, slash command host impl | ~200 |

→ Risk: **High** (many method interdependencies, needs careful split)

### Phase 11: Tighten field visibility

**Principle #4: Strong typing, encapsulation**

After Phase 9, reduce `pub` fields on `FullscreenState`:
- Keep `pub`: `transcript`, `mode`, `viewport`, `projection` (needed by `frame.rs`)
- Make `pub(crate)`: `input`, `cursor`, `size`, `footer`, `title`, `input_placeholder`
- Make private: `dirty`, `tick_count`, `submitted_inputs`, `should_quit`
- Add accessor methods where needed

### Phase 12: Reduce cloning in controller

**Principle #1: Minimize cloning**

Review the 58 `.clone()` calls in the controller. Key targets:
- `self.options.initial_message.clone()` → take with `Option::take`
- `self.options.initial_messages.clone()` → `drain` or `mem::take`
- String clones in menu building → borrow where possible
- `session_id.clone()` throughout → pass `&str` instead

---

## After Refactoring

### `crates/tui/src/fullscreen/` (target: no file > 600 lines)

| File | Est. Lines | Responsibility |
|------|-----------|----------------|
| `mod.rs` | ~30 | Wiring only |
| `types.rs` | ~150 | All public types |
| `runtime.rs` | ~500 | `FullscreenState` struct + `apply_command` + `run_with_channels` |
| `events.rs` | ~300 | Key/mouse/paste/tick handling |
| `input.rs` | ~100 | Text editing |
| `navigation.rs` | ~180 | Block focus, page movement |
| `search.rs` | ~100 | Search mode |
| `menus.rs` | ~200 | Slash + select menus |
| `streaming.rs` | ~200 | Turn/tool tracking |
| `tool_format.rs` | ~350 | Tool formatting (shared, deduped) |
| `frame.rs` | ~680 | Frame rendering |
| `projection.rs` | ~580 | Row projection |
| `transcript/` | ~440 | Block tree |
| `layout.rs` | ~170 | Layout calc |
| `viewport.rs` | ~150 | Scroll state |
| `terminal.rs` | ~140 | Terminal I/O |
| `scheduler.rs` | ~100 | Render batching |
| `renderer.rs` | ~85 | Diff renderer |
| `tests.rs` | ~930 | All tests |

### `crates/cli/src/fullscreen/` (target: no file > 350 lines)

| File | Est. Lines | Responsibility |
|------|-----------|----------------|
| `mod.rs` | ~50 | Entry + re-exports |
| `controller.rs` | ~300 | Core controller loop |
| `session.rs` | ~350 | DB + transcript |
| `auth.rs` | ~150 | Auth flows |
| `models.rs` | ~300 | Model management |
| `turns.rs` | ~300 | Streaming turn loop |
| `formatting.rs` | ~200 | Shared formatters |
| `menus.rs` | ~200 | Menu dispatch |

## Execution Priority

| Priority | Phases | Impact | Risk |
|----------|--------|--------|------|
| **P0** | 1, 2 | Types + dedup formatting | Low |
| **P1** | 3, 4, 9 | Input + menus + tests out | Low |
| **P2** | 5, 6, 7, 8 | Navigation + search + events + streaming | Medium |
| **P3** | 10 | CLI controller split | High |
| **P4** | 11, 12 | Visibility + clone reduction | Low |
