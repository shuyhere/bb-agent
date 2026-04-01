Build SelectList component and overlay system for BB-Agent TUI.

Work in `~/BB-Agent/crates/tui/src/`. Read AGENTS.md for project context.

## Task 1: Create `select_list.rs`

A selectable list with keyboard navigation.

```rust
pub struct SelectItem {
    pub label: String,
    pub detail: Option<String>,  // secondary text (dimmed)
    pub value: String,           // return value on select
}

pub struct SelectList {
    items: Vec<SelectItem>,
    filtered: Vec<usize>,       // indices into items after filtering
    selected: usize,            // index in filtered
    scroll_offset: usize,
    max_visible: usize,
    search: String,
}

impl SelectList {
    pub fn new(items: Vec<SelectItem>, max_visible: usize) -> Self;
    pub fn render(&self, width: u16) -> Vec<String>;
    pub fn handle_key(&mut self, key: KeyEvent) -> SelectAction;
    pub fn set_search(&mut self, query: &str);
}

pub enum SelectAction {
    None,
    Selected(String),  // value of selected item
    Cancelled,
}
```

### Key bindings
- Up/Down — move selection
- PageUp/PageDown — page scroll
- Home/End — first/last
- Enter — select current item
- Escape — cancel
- Type characters → filter list (fuzzy match)

### Rendering
- Show `>` marker on selected item
- Selected item highlighted (e.g., reverse video or bright)
- Unselected items normal
- Show scroll indicators if list is longer than max_visible
- Show search query at top if non-empty

## Task 2: Create `model_selector.rs`

Use SelectList to implement `/model` command:
- Load models from provider registry
- Show provider, model id, context window, thinking
- Fuzzy search filtering
- Return selected model info

## Task 3: Create `session_selector.rs`

Use SelectList to implement `/resume` command:
- Load sessions from SQLite
- Show session name/id, entry count, last updated
- Select to resume

### Build and test
```
cd ~/BB-Agent && cargo build
```
