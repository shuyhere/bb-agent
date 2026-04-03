#!/usr/bin/env bash
set -euo pipefail

SESSION="bb-tui-polish"
SRC_DIR="$HOME/BB-Agent"
TASK_DIR="$SRC_DIR/.bb-agent/tasks/tui-polish"

tmux kill-session -t "$SESSION" 2>/dev/null || true

# Each phase runs sequentially on master, committing before the next starts.
# We use a single worktree at /tmp/bb-tui-polish that tracks master.
WORKTREE="/tmp/bb-tui-polish"
BRANCH="worktree/tui-polish"

cd "$SRC_DIR"
git worktree remove "$WORKTREE" --force 2>/dev/null || true
git branch -D "$BRANCH" 2>/dev/null || true
git worktree add -b "$BRANCH" "$WORKTREE" HEAD

tmux new-session -d -s "$SESSION" -x 200 -y 50

# Build the sequential script that runs all phases
cat > /tmp/bb-tui-polish-runner.sh << 'RUNNER'
#!/usr/bin/env bash
set -euo pipefail

WORKTREE="/tmp/bb-tui-polish"
SRC_DIR="$HOME/BB-Agent"
TASK_DIR="$SRC_DIR/.bb-agent/tasks/tui-polish"

cd "$WORKTREE"

run_phase() {
    local phase_num="$1"
    local phase_desc="$2"
    
    echo ""
    echo "============================================"
    echo "  Phase $phase_num: $phase_desc"
    echo "============================================"
    echo ""
    
    pi -p "You are working in a git worktree at /tmp/bb-tui-polish (branch worktree/tui-polish).

Read the plan at $TASK_DIR/PLAN.md and execute ONLY Phase $phase_num: $phase_desc.

Rules:
1. This is a STRUCTURAL refactor only. No behavior changes.
2. Move code exactly as described in the plan. Do not rewrite logic.
3. After moving, update imports in the source file and the new file.
4. Update mod.rs to declare and re-export the new module.
5. Verify: cargo build -q -p bb-tui && cargo test -q -p bb-tui
6. If Phase touches fullscreen_entry.rs, also verify: cargo build -q -p bb-cli
7. Only stage files YOU changed. Do not run cargo fmt on unrelated files.
8. Commit with message: 'phase $phase_num: $phase_desc'
9. Do NOT modify any file that the plan does not specify for this phase.
10. If a previous phase already extracted something, do not re-extract it."

    # Verify it compiled
    echo "Verifying phase $phase_num..."
    cd "$WORKTREE"
    cargo build -q -p bb-tui 2>/dev/null && echo "  ✓ bb-tui builds" || echo "  ✗ bb-tui FAILED"
    cargo test -q -p bb-tui 2>/dev/null && echo "  ✓ bb-tui tests pass" || echo "  ✗ bb-tui tests FAILED"
}

# Run phases sequentially
run_phase 1 "extract types to types.rs"
run_phase 2 "extract tool formatting to tool_format.rs"
run_phase 9 "extract tests to tests.rs"
run_phase 3 "extract input editing to input.rs"
run_phase 4 "extract menus to menus.rs"
run_phase 6 "extract search to search.rs"
run_phase 5 "extract focus/navigation to navigation.rs"
run_phase 7 "extract key/event handlers to events.rs"
run_phase 8 "extract streaming/turn state to streaming.rs"
run_phase 10 "split fullscreen_entry.rs into fullscreen/ modules"

echo ""
echo "============================================"
echo "  ALL PHASES COMPLETE"
echo "============================================"
echo ""
git log --oneline -12
echo ""
wc -l crates/tui/src/fullscreen/*.rs crates/tui/src/fullscreen/transcript/mod.rs | sort -rn | head -20
RUNNER

chmod +x /tmp/bb-tui-polish-runner.sh

tmux send-keys -t "$SESSION" "bash /tmp/bb-tui-polish-runner.sh" Enter

echo "Launched sequential TUI polish in tmux session: $SESSION"
echo "Monitor with: tmux attach -t $SESSION"
