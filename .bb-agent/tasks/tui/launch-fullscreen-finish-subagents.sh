#!/usr/bin/env bash
set -euo pipefail

ROOT="/home/shuyhere/BB-Agent"
BASE="/tmp/bb-fullscreen-final"
LOG_DIR="$BASE/logs"
SESSION="bb-fullscreen-final"
TASKS="$ROOT/.bb-agent/tasks/tui"

command -v tmux >/dev/null
command -v pi >/dev/null

mkdir -p "$BASE" "$LOG_DIR"

ensure_worktree() {
  local branch="$1" dir="$2"
  [ -e "$dir" ] && { echo "reusing: $dir"; return; }
  if git -C "$ROOT" show-ref --verify --quiet "refs/heads/$branch" 2>/dev/null; then
    git -C "$ROOT" worktree add "$dir" "$branch"
  else
    git -C "$ROOT" worktree add -b "$branch" "$dir" master
  fi
}

launch_window() {
  local window="$1" dir="$2" task="$3" log="$4"
  local cmd="cd '$dir' && PROMPT=\"\$(cat '$task')\" && pi -p --no-session \"\$PROMPT\" 2>&1 | tee '$log'; printf '\n[%s] done.\n' '$window'; exec bash"
  if tmux has-session -t "$SESSION" 2>/dev/null; then
    tmux new-window -t "$SESSION" -n "$window" "$cmd"
  else
    tmux new-session -d -s "$SESSION" -n "$window" "$cmd"
  fi
}

tmux kill-session -t "$SESSION" 2>/dev/null || true

ensure_worktree r35-shared-fullscreen-controls "$BASE/r35-controls"
ensure_worktree r36-shared-fullscreen-streaming "$BASE/r36-streaming"
ensure_worktree r37-shared-fullscreen-runtime-mapping "$BASE/r37-runtime"
ensure_worktree r38-fullscreen-cleanup "$BASE/r38-cleanup"

launch_window controls  "$BASE/r35-controls"  "$TASKS/r35-shared-fullscreen-controls.md"        "$LOG_DIR/r35.log"
launch_window streaming "$BASE/r36-streaming" "$TASKS/r36-shared-fullscreen-streaming.md"       "$LOG_DIR/r36.log"
launch_window runtime   "$BASE/r37-runtime"   "$TASKS/r37-shared-fullscreen-runtime-mapping.md" "$LOG_DIR/r37.log"
launch_window cleanup   "$BASE/r38-cleanup"   "$TASKS/r38-fullscreen-cleanup.md"                "$LOG_DIR/r38.log"

printf 'launched tmux session: %s (4 windows)\n' "$SESSION"
printf 'attach: tmux attach -t %s\n' "$SESSION"
