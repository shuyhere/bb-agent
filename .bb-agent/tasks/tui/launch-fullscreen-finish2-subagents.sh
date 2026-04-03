#!/usr/bin/env bash
set -euo pipefail

ROOT="/home/shuyhere/BB-Agent"
BASE="/tmp/bb-fullscreen-finish"
LOG_DIR="$BASE/logs"
SESSION="bb-fullscreen-finish"
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

ensure_worktree r39-fullscreen-accepts-integration "$BASE/r39-integration"
ensure_worktree r40-shared-fullscreen-streaming-final "$BASE/r40-streaming"
ensure_worktree r41-shared-fullscreen-runtime-final "$BASE/r41-runtime"
ensure_worktree r42-fullscreen-terminal-verification "$BASE/r42-verify"

launch_window integrate "$BASE/r39-integration" "$TASKS/r39-fullscreen-accepts-integration.md"      "$LOG_DIR/r39.log"
launch_window streaming "$BASE/r40-streaming"   "$TASKS/r40-shared-fullscreen-streaming-final.md"   "$LOG_DIR/r40.log"
launch_window runtime   "$BASE/r41-runtime"     "$TASKS/r41-shared-fullscreen-runtime-final.md"     "$LOG_DIR/r41.log"
launch_window verify    "$BASE/r42-verify"      "$TASKS/r42-fullscreen-terminal-verification.md"    "$LOG_DIR/r42.log"

printf 'launched tmux session: %s (4 windows)\n' "$SESSION"
printf 'attach: tmux attach -t %s\n' "$SESSION"
