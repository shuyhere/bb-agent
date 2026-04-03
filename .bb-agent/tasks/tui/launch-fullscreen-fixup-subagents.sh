#!/usr/bin/env bash
set -euo pipefail

ROOT="/home/shuyhere/BB-Agent"
BASE="/tmp/bb-fullscreen-next"
LOG_DIR="$BASE/logs"
SESSION="bb-fullscreen-next"
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

ensure_worktree r30-unify-fullscreen-stack "$BASE/r30-unify"
ensure_worktree r31-projector-viewport-integration "$BASE/r31-projector"
ensure_worktree r32-transcript-controls "$BASE/r32-controls"
ensure_worktree r33-fullscreen-streaming "$BASE/r33-streaming"
ensure_worktree r34-fullscreen-runtime-mapping "$BASE/r34-runtime"

launch_window unify     "$BASE/r30-unify"     "$TASKS/r30-unify-fullscreen-stack.md"        "$LOG_DIR/r30.log"
launch_window projector "$BASE/r31-projector" "$TASKS/r31-projector-viewport-integration.md" "$LOG_DIR/r31.log"
launch_window controls  "$BASE/r32-controls"  "$TASKS/r32-transcript-controls.md"           "$LOG_DIR/r32.log"
launch_window streaming "$BASE/r33-streaming" "$TASKS/r33-fullscreen-streaming.md"          "$LOG_DIR/r33.log"
launch_window runtime   "$BASE/r34-runtime"   "$TASKS/r34-fullscreen-runtime-mapping.md"    "$LOG_DIR/r34.log"

printf 'launched tmux session: %s (5 windows)\n' "$SESSION"
printf 'attach: tmux attach -t %s\n' "$SESSION"
