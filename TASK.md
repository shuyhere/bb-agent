# A5: Build tree selector for /tree navigation

Working dir: `/tmp/bb-final/a5-tree-selector/`
BB-Agent Rust project.

## Task: Create `crates/tui/src/tree_selector.rs`

Build an interactive tree navigation component for the `/tree` command. This is pi's killer feature — navigate the session tree, select any node, and continue from there.

### Tree display format
```
├─ user: "Hello, can you help..."
│  └─ assistant: "Of course! I can..."
│     ├─ user: "Let's try approach A..."
│     │  └─ assistant: "For approach A..."
│     │     └─ [compaction: 12k tokens]
│     │        └─ user: "That worked..."  ← active
│     └─ user: "Actually, approach B..."
│        └─ assistant: "For approach B..."
```

### Implementation

```rust
use bb_session::store::EntryRow;
use bb_session::tree::TreeNode;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub struct TreeSelector {
    nodes: Vec<FlatNode>,     // flattened tree for display
    selected: usize,
    scroll_offset: usize,
    max_visible: usize,
    active_leaf: Option<String>,
    filter: TreeFilter,
}

#[derive(Clone)]
struct FlatNode {
    entry_id: String,
    entry_type: String,
    depth: usize,
    preview: String,       // truncated message preview
    is_active: bool,       // is this the current leaf?
    has_children: bool,
    timestamp: String,
}

pub enum TreeFilter {
    All,
    UserOnly,
}

pub enum TreeAction {
    None,
    Selected(String),    // entry_id selected
    Cancelled,
}

impl TreeSelector {
    pub fn new(tree: Vec<TreeNode>, active_leaf: Option<&str>, max_visible: usize) -> Self;
    pub fn render(&self, width: u16) -> Vec<String>;
    pub fn handle_key(&mut self, key: KeyEvent) -> TreeAction;
}
```

### Flatten the tree
Convert the recursive `TreeNode` into a flat list with depth info:

```rust
fn flatten(nodes: &[TreeNode], depth: usize, active_leaf: Option<&str>) -> Vec<FlatNode> {
    let mut flat = Vec::new();
    for node in nodes {
        let preview = extract_preview(&node);
        let is_active = active_leaf.map(|l| l == node.entry_id).unwrap_or(false);
        flat.push(FlatNode {
            entry_id: node.entry_id.clone(),
            entry_type: node.entry_type.clone(),
            depth,
            preview,
            is_active,
            has_children: !node.children.is_empty(),
            timestamp: node.timestamp.clone(),
        });
        flat.extend(flatten(&node.children, depth + 1, active_leaf));
    }
    flat
}
```

### Extract preview text from entry
Parse the entry payload JSON to get a short preview:
- `message` with `user` role → first 60 chars of text
- `message` with `assistant` role → "assistant: " + first 40 chars
- `compaction` → "[compaction: {tokens_before} tokens]"
- `branch_summary` → "[branch summary]"
- `model_change` → "[model: {model_id}]"
- other → "[{type}]"

### Rendering
Each line:
```
{indent}{connector} {type_icon}: {preview}  {active_marker}
```

Where:
- `indent` = `│  ` repeated by depth
- `connector` = `├─` for non-last child, `└─` for last child
- `type_icon` = colored by type (user=blue, assistant=green, compaction=gray)
- `active_marker` = `← active` if this is the current leaf
- Selected line gets reverse video or `>` marker

### Key bindings
- Up/Down — navigate
- Enter — select (return entry_id)
- Escape — cancel
- Ctrl+U — toggle user-only filter
- Home/End — first/last

### Wire into interactive mode

In `crates/cli/src/interactive.rs`, when `/tree` is entered:
1. Get tree from `bb_session::tree::get_tree()`
2. Get current leaf from session
3. Create `TreeSelector`
4. Enter selection loop
5. On selection: call session navigation (branch to parent if user msg, branch to node if other)

### Build and test
```bash
cd /tmp/bb-final/a5-tree-selector
cargo build && cargo test
git add -A && git commit -m "A5: tree selector for /tree navigation"
```
