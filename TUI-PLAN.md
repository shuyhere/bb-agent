# BB-Agent TUI Build Plan

> Based on line-by-line study of [pi-mono/packages/tui](https://github.com/badlogic/pi-mono/tree/main/packages/tui)
> (10,724 lines) and [pi-mono/packages/coding-agent/src/modes/interactive](https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent/src/modes/interactive)
> (~7,332 lines of components + ~4,200 lines of interactive-mode.ts).

---

## How pi's TUI works

Pi's TUI is **scrollback-based** (not fullscreen). It writes to the terminal's scrollback buffer
like a normal CLI program, only moving the cursor back to update the editor area at the bottom.
This preserves natural scrolling and search.

### Architecture (3 layers)

```
Layer 1: Terminal abstraction        (terminal.ts — 360 lines)
  │  Raw stdin/stdout, cursor, raw mode, Kitty protocol,
  │  synchronized output, resize events
  │
Layer 2: TUI framework               (tui.ts — 1200 lines)
  │  Component tree, Container, differential rendering,
  │  overlay system, focus management, cursor marker
  │
Layer 3: Components                   (components/* — ~4800 lines)
     Editor (2230), Markdown (824), SelectList (229),
     Input (503), Text (106), Box (137), Spacer (28),
     Image (104), Loader (55), etc.
```

### Layer 1: Terminal (`terminal.ts` — 360 lines)

```
ProcessTerminal
  ├── start(onInput, onResize)     # enable raw mode, Kitty protocol, bracketed paste
  ├── stop()                       # restore terminal state
  ├── write(data)                  # write to stdout (with optional debug log)
  ├── columns / rows               # terminal dimensions
  ├── hideCursor / showCursor
  ├── clearLine / clearScreen
  ├── setTitle(title)
  └── drainInput()                 # drain stdin before exit (SSH Kitty leak fix)
```

Key details:
- Enables **Kitty keyboard protocol** for precise key detection (`CSI > 1 u`)
- Enables **bracketed paste** (`CSI ? 2004 h`) for proper paste handling
- Falls back to **modifyOtherKeys** (`CSI > 4 ; 2 m`) when Kitty not available
- Uses `StdinBuffer` for buffered input parsing (handles split escape sequences)
- All writes go through a single `write()` for optional debug logging

### Layer 2: TUI core (`tui.ts` — 1200 lines)

```
Component interface:
  render(width: number) → string[]     # render to lines
  handleInput?(data: string)           # keyboard input when focused
  invalidate()                         # clear caches

Container extends Component:
  children: Component[]
  addChild / removeChild / clear

TUI extends Container:
  terminal: Terminal
  start() / stop()
  setFocus(component)
  requestRender()
  showOverlay(component, options) → OverlayHandle
  hideOverlay()
  addInputListener(fn)
```

**Differential rendering algorithm** (`doRender()`):
1. Render all components → `newLines[]`
2. Composite overlays on top (if any)
3. Extract cursor position (search for `CURSOR_MARKER`)
4. Apply line resets (append `ESC[0m` to each line)
5. Compare with `previousLines[]`:
   - First render → output all lines
   - Width changed → full clear + re-render
   - Height changed → full clear + re-render
   - Otherwise → find first changed line, re-render from there
6. Wrap output in synchronized output (`CSI ? 2026 h/l`)
7. Position hardware cursor for IME
8. Store `previousLines` for next render

**Overlay system**:
- Overlays render on top of base content (composited per-line)
- Focus stack: overlays capture keyboard focus
- OverlayHandle: hide/show/focus/unfocus
- Configurable positioning (anchor, percentage, margin)

### Layer 3: Components

#### Editor (`components/editor.ts` — 2230 lines) — the biggest component

```
Features:
  ├── Multi-line editing with word wrap
  ├── Cursor movement (arrows, Home/End, Ctrl+A/E, word-jump)
  ├── Selection (Shift+arrows)
  ├── Kill ring (Ctrl+K, Ctrl+Y, Alt+Y)
  ├── Undo/redo (Ctrl+Z, Ctrl+Shift+Z)
  ├── Bracketed paste handling (large paste → collapsed marker)
  ├── Autocomplete (file paths, slash commands, fuzzy matching)
  ├── History (Up/Down when on first/last line)
  ├── Theme support (border, prompt, text colors)
  ├── Scrolling within editor area
  ├── CURSOR_MARKER emission for IME
  └── Submit on Enter (or Alt+Enter for newline)
```

#### Markdown (`components/markdown.ts` — 824 lines)

```
Features:
  ├── Parse markdown via `marked` library
  ├── Headings, bold, italic, strikethrough, underline
  ├── Code blocks with syntax highlighting (via `cli-highlight`)
  ├── Inline code
  ├── Block quotes with border
  ├── Lists (ordered + unordered, nested)
  ├── Links with URL display
  ├── Horizontal rules
  ├── Tables
  ├── Word-wrap with ANSI-aware width calculation
  └── Theme support (colors for each element)
```

#### SelectList (`components/select-list.ts` — 229 lines)

```
Features:
  ├── Vertical list with keyboard navigation
  ├── Up/Down/PageUp/PageDown/Home/End
  ├── Enter to select, Escape to cancel
  ├── Scrollable with visible window
  ├── Theme support (selected/unselected colors)
  └── Fuzzy search filtering
```

#### Other components

| Component | Lines | Purpose |
|-----------|-------|---------|
| Input | 503 | Single-line text input with cursor |
| Box | 137 | Bordered box container |
| Text | 106 | Static text display |
| Image | 104 | Terminal image display (Kitty/iTerm2/Sixel) |
| Loader | 55 | Animated spinner |
| CancellableLoader | 40 | Loader with cancel hint |
| Spacer | 28 | Empty space |
| TruncatedText | 65 | Text truncated to width |
| SettingsList | 250 | Key-value settings editor |

### Supporting modules

| Module | Lines | Purpose |
|--------|-------|---------|
| utils.ts | 1068 | `visibleWidth`, `truncateToWidth`, `wrapTextWithAnsi`, ANSI parsing |
| keys.ts | 1356 | Key sequence parsing, Kitty protocol decoding, `matchesKey()` |
| autocomplete.ts | 773 | File path completion, slash command completion, fuzzy matching |
| stdin-buffer.ts | 386 | Buffered stdin reader (handles split escape sequences) |
| terminal-image.ts | 381 | Image rendering (Kitty/iTerm2/Sixel protocols) |
| keybindings.ts | 244 | Configurable key bindings |
| fuzzy.ts | 133 | Fuzzy string matching |
| kill-ring.ts | 46 | Emacs-style kill ring |
| undo-stack.ts | 28 | Undo/redo stack |

---

## What coding-agent builds on top

The coding-agent's interactive mode (`interactive-mode.ts`, ~4200 lines) uses
the TUI framework to build the full agent UI:

### App-level components (7332 lines)

| Component | Lines | What it does |
|-----------|-------|-------------|
| tree-selector | 1239 | Tree navigation with fold/unfold, search, branch display |
| session-selector | 1010 | Session picker with delete, search, rename |
| config-selector | 592 | Package resource enable/disable |
| settings-selector | 432 | Settings editor |
| armin | 382 | Easter egg game |
| scoped-models-selector | 346 | Ctrl+P model cycling setup |
| model-selector | 337 | `/model` command with fuzzy search |
| tool-execution | 328 | Tool call display with streaming diff |
| bash-execution | 218 | Bash output display |
| footer | 220 | Status bar (model, tokens, git, extension status) |
| session-selector-search | 194 | Cross-session search |
| login-dialog | 178 | OAuth login UI |
| daxnuts | 164 | Easter egg |
| diff | 147 | Inline diff display |
| extension-editor | 147 | Extended editor for extensions |
| user-message-selector | 143 | Pick a previous user message |
| assistant-message | 130 | Render assistant message with markdown |
| oauth-selector | 121 | Provider login/logout picker |
| extension-selector | 107 | Extension picker |

---

## Build plan for BB-Agent TUI

### Principles

1. **Port concepts, not code** — pi is TypeScript, we're Rust. Port the design, not line-by-line.
2. **Incremental** — each phase produces a working `bb` that's better than before.
3. **Scrollback-based** — same approach as pi. Not ratatui fullscreen.
4. **crossterm** — use crossterm for terminal abstraction (equivalent to pi's Terminal).

### Why not ratatui?

After studying pi's approach, ratatui is the wrong choice. Pi uses scrollback-based rendering
which preserves natural terminal scrolling and search. Ratatui is fullscreen/alternate-screen.
We should use **crossterm directly** (low-level terminal ops) and build our own differential
rendering, just like pi does.

---

### Phase T1: Terminal + basic differential rendering (Week 1)

**Goal**: Replace current raw-mode editor with proper terminal management.

Build:
- `tui/terminal.rs` — Terminal trait + ProcessTerminal (crossterm-based)
  - start/stop with raw mode
  - Kitty keyboard protocol (or fallback)
  - Bracketed paste mode
  - Synchronized output (`CSI ?2026h/l`)
  - write, columns, rows, cursor ops
- `tui/component.rs` — Component trait + Container
  - `render(width) → Vec<String>`
  - `handle_input(data)`
  - `invalidate()`
- `tui/renderer.rs` — Differential renderer
  - Compare newLines vs previousLines
  - Find first changed line, re-render from there
  - Full re-render on width/height change
  - Wrap in synchronized output
- `tui/utils.rs` — `visible_width()`, `truncate_to_width()`, ANSI-aware string ops

**Deliverable**: `bb` renders chat output with differential updates, no flicker.

---

### Phase T2: Editor component (Week 2)

**Goal**: Proper multi-line editor replacing the basic line reader.

Build:
- `tui/editor.rs` — Full editor component
  - Multi-line editing with word wrap
  - Cursor movement (arrows, Home/End, Ctrl+A/E, word-jump with Alt+Left/Right)
  - Backspace, Delete, Ctrl+K (kill to EOL), Ctrl+U (clear line)
  - History (Up/Down)
  - Submit on Enter
  - Ctrl+C to clear/abort, Ctrl+D to exit
  - Proper cursor positioning with CURSOR_MARKER
  - Border/prompt rendering

**Deliverable**: `bb` has a proper editor at the bottom that feels like a real CLI input.

---

### Phase T3: Markdown rendering (Week 3)

**Goal**: Assistant output rendered with markdown formatting.

Build:
- `tui/markdown.rs` — Markdown component
  - Parse markdown (use `pulldown-cmark` crate)
  - Headings (bold + colored)
  - Bold, italic, strikethrough
  - Code blocks with language label (use `syntect` for highlighting)
  - Inline code (background colored)
  - Block quotes with border
  - Lists (ordered + unordered)
  - Links
  - Horizontal rules
  - Word-wrap with ANSI-aware width
- Update assistant message rendering to use Markdown component

**Deliverable**: Assistant output looks like properly formatted markdown.

---

### Phase T4: SelectList + overlays (Week 4)

**Goal**: Interactive selectors for models, sessions, tree.

Build:
- `tui/select_list.rs` — SelectList component
  - Keyboard navigation (Up/Down/PgUp/PgDn/Home/End)
  - Enter to select, Escape to cancel
  - Scrollable window
  - Optional fuzzy search filtering
- `tui/overlay.rs` — Overlay system
  - Render overlay on top of base content (composited per-line)
  - Focus management (overlay captures input)
  - Configurable positioning
- `tui/model_selector.rs` — `/model` selector using SelectList
- `tui/session_selector.rs` — `/resume` selector using SelectList

**Deliverable**: `/model` and `/resume` show interactive pickers.

---

### Phase T5: Tool execution display (Week 5)

**Goal**: Tool calls and results displayed properly with streaming.

Build:
- `tui/tool_display.rs` — Tool execution component
  - Show tool name + arguments (collapsible)
  - Streaming diff display for edit tool
  - Bash output with syntax coloring
  - Read file with line numbers
  - Expand/collapse with Ctrl+O
  - Status indicator (⏳ running, ✓ done, ✗ error)
- Update the agent loop to emit component events during tool execution

**Deliverable**: Tool calls display like pi does — compact by default, expandable.

---

### Phase T6: Footer + status bar (Week 6)

**Goal**: Persistent status information at the bottom.

Build:
- `tui/footer.rs` — Footer component
  - Current model name
  - Token usage (input/output/cache)
  - Context usage percentage
  - Git branch (via `git rev-parse`)
  - Cost tracking
  - Keyboard shortcut hints
- Wire footer into main TUI layout

**Deliverable**: Status bar showing model + context usage + cost at bottom.

---

### Phase T7: Tree selector (Week 7)

**Goal**: `/tree` navigation with full tree display.

Build:
- `tui/tree_selector.rs` — Tree navigation component
  - Render session tree with indentation
  - Fold/unfold branches (Ctrl+Left/Right)
  - Mark active leaf
  - Show entry types (user, assistant, compaction, branch_summary)
  - Navigate with Up/Down, select with Enter
  - Search/filter
  - User-only toggle (Ctrl+U)

**Deliverable**: `/tree` shows interactive tree navigator.

---

### Phase T8: Autocomplete + file paths (Week 8)

**Goal**: Editor autocomplete for file paths and slash commands.

Build:
- `tui/autocomplete.rs` — Autocomplete system
  - File path completion (scan filesystem)
  - Slash command completion
  - Fuzzy matching
  - Dropdown display below/above cursor
- Wire into editor component

**Deliverable**: Tab completion for file paths and `/` commands in editor.

---

## Summary: what to build and total effort

| Phase | Component | Effort | pi equivalent lines |
|-------|-----------|--------|-------------------|
| T1 | Terminal + renderer | 1 week | terminal.ts (360) + tui.ts (1200) + utils.ts (1068) |
| T2 | Editor | 1 week | editor.ts (2230) |
| T3 | Markdown | 1 week | markdown.ts (824) |
| T4 | SelectList + overlays | 1 week | select-list.ts (229) + overlay code in tui.ts |
| T5 | Tool display | 1 week | tool-execution.ts (328) + diff.ts (147) + bash-execution.ts (218) |
| T6 | Footer | 0.5 week | footer.ts (220) |
| T7 | Tree selector | 1 week | tree-selector.ts (1239) |
| T8 | Autocomplete | 1 week | autocomplete.ts (773) + fuzzy.ts (133) |

**Total: ~7.5 weeks for full TUI parity with pi.**

### Priority order (what gives most value first)

1. **T1 + T2** (terminal + editor) — makes `bb` actually usable as an interactive agent
2. **T3** (markdown) — makes output readable
3. **T5** (tool display) — makes tool execution visible
4. **T6** (footer) — status awareness
5. **T4** (selectors) — interactive `/model`, `/resume`
6. **T7** (tree) — tree navigation
7. **T8** (autocomplete) — productivity boost

---

## Key Rust crates to use

| Need | Crate | Why |
|------|-------|-----|
| Terminal ops | `crossterm` | Raw mode, cursor, colors, events (already in deps) |
| Markdown parsing | `pulldown-cmark` | Fast, pure Rust, well-maintained |
| Syntax highlighting | `syntect` | Code block highlighting |
| Unicode width | `unicode-width` | Correct `visible_width()` for CJK, emoji |
| Fuzzy matching | `fuzzy-matcher` or custom | For autocomplete and selectors |

---

## What NOT to port from pi

| pi feature | Skip for now | Reason |
|------------|-------------|--------|
| Image display (Kitty/iTerm2/Sixel) | Yes | Niche, add later |
| Kitty keyboard protocol | Yes | crossterm handles key detection |
| SettingsList component | Yes | Use CLI flags instead |
| Easter eggs (armin, daxnuts) | Yes | Fun but not core |
| Config selector | Yes | Use settings.json |
| Extension editor/selector | Yes | Not needed until plugin system is mature |
| Login dialog (OAuth flow) | Yes | `bb login` CLI is sufficient |
| Custom editor replacement API | Yes | Extension feature, defer |
| Scoped models selector | Yes | `bb --models` CLI is sufficient |
