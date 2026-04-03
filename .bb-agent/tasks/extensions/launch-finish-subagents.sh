#!/usr/bin/env bash
set -euo pipefail

SESSION="bb-finish-ext"
BASE_DIR="/tmp/bb-finish-ext"
SRC_DIR="$HOME/BB-Agent"
BRANCH_BASE="worktree/finish-ext"

# Clean up any previous run
tmux kill-session -t "$SESSION" 2>/dev/null || true
rm -rf "$BASE_DIR"
mkdir -p "$BASE_DIR"

# Task definitions: name → task file → branch suffix
declare -A TASKS
TASKS[r50]="r50-interactive-ui-consume.md"
TASKS[r51]="r51-package-auto-install.md"
TASKS[r52]="r52-glob-filter-patterns.md"
TASKS[r53]="r53-interactive-dialogs.md"
TASKS[r54]="r54-orchestration-stubs.md"

# Create tmux session (detached)
tmux new-session -d -s "$SESSION" -x 200 -y 50

FIRST=true
for NAME in r50 r51 r52 r53 r54; do
    TASK_FILE="${TASKS[$NAME]}"
    WORKTREE="$BASE_DIR/$NAME"
    BRANCH="${BRANCH_BASE}-${NAME}"
    TASK_PATH="$SRC_DIR/.bb-agent/tasks/extensions/$TASK_FILE"

    # Create worktree
    cd "$SRC_DIR"
    git worktree add -b "$BRANCH" "$WORKTREE" HEAD 2>/dev/null || {
        git branch -D "$BRANCH" 2>/dev/null || true
        git worktree add -b "$BRANCH" "$WORKTREE" HEAD
    }

    # Read the task content
    TASK_CONTENT=$(cat "$TASK_PATH")

    # Build the prompt
    PROMPT="You are working in a git worktree at $WORKTREE (branch $BRANCH).
The main repo is at $SRC_DIR. Do NOT touch the main repo directly.

Your task:
$TASK_CONTENT

Instructions:
1. Read the task file above carefully.
2. Make all changes in the worktree.
3. Run the specified build and test commands to verify.
4. Commit your changes with a descriptive message.
5. Exit when done.

Important:
- Only modify files listed in the task.
- Do not run cargo fmt on files you did not change.
- If cargo fmt touches unrelated files, revert them before committing.
- Make sure the commit only contains your intended changes."

    if [ "$FIRST" = true ]; then
        tmux send-keys -t "$SESSION" "cd $WORKTREE && pi -p \"$( echo "$PROMPT" | sed "s/'/'\\\\''/g" )\"" Enter
        FIRST=false
    else
        tmux new-window -t "$SESSION" -n "$NAME"
        tmux send-keys -t "$SESSION:$NAME" "cd $WORKTREE && pi -p \"$( echo "$PROMPT" | sed "s/'/'\\\\''/g" )\"" Enter
    fi

    echo "✓ Launched $NAME in $WORKTREE on branch $BRANCH"
done

echo ""
echo "All 5 subagents launched in tmux session: $SESSION"
echo "Monitor with: tmux attach -t $SESSION"
echo "List windows: tmux list-windows -t $SESSION"
