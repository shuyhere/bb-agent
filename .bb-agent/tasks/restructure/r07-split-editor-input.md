# Task: split `crates/tui/src/editor/input.rs` (pass 2)

Worktree: `/tmp/bb-restructure/r07-editor-input`
Branch: `r07-split-editor-input`

## Goal
`editor/input.rs` is 817 lines mixing: slash menus, file menus, text selection, cursor navigation, history browsing, undo/redo, kill ring, character insertion/deletion, and key dispatch.

Split by responsibility into smaller focused files.

## Principles
- One file, one responsibility
- Keep `mod.rs` routing only (already is)
- Preserve all existing tests in `tests.rs`

## Likely split
Read `input.rs` carefully and split:

1. `editing.rs` — insert_char, insert_str, backspace, delete, new_line, kill_to_end, kill_to_start, delete_word_backward (basic text mutation)
2. `navigation.rs` — move_left, move_right, move_up, move_down, move_to_line_start, move_to_line_end, word_left, word_right, navigate_history (cursor movement)
3. `selection.rs` — has_selection, selection_range, selected_text, delete_selection, clear_selection, select_all, ensure_anchor (text selection)
4. `menus.rs` — slash_query, update_slash_menu, accept_slash_selection, file_query, scan_files, update_file_menu, accept_file_selection (autocomplete menus)
5. `key_dispatch.rs` — the large handle_input match and handle_raw_input (key event routing)

Keep `input.rs` as a thin re-export or remove it and add the new files directly to `editor/mod.rs`.

All new files should use `impl Editor` blocks with `pub(super)` methods.

## Important
- The current `input.rs` has a `base64_encode` helper function and `submit`, `undo`, `redo`, `yank`, `yank_pop`, `snapshot`, `restore`, `push_undo` methods.
- Put `base64_encode` near clipboard/yank code.
- Put undo/redo/snapshot in `editing.rs` or a separate `history.rs`.

## Constraints
- Do NOT change behavior or key bindings.
- Do NOT modify tests (they should still pass as-is).
- Preserve `pub(super)` visibility.

## Verification
```
cargo build -q -p bb-tui
cargo test -q -p bb-tui
```

## Finish
```
git add -A
git commit -m "split editor/input.rs into focused responsibility files"
```

Report: changed files, verification results, commit hash.
