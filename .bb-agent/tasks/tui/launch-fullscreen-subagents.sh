#!/usr/bin/env bash
set -euo pipefail

ROOT="/home/shuyhere/BB-Agent"
BASE="/tmp/bb-fullscreen"
LOG_DIR="$BASE/logs"
SESSION="bb-fullscreen"
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

ensure_worktree r24-fullscreen-foundation "$BASE/r24-foundation"
ensure_worktree r25-transcript-block-model "$BASE/r25-block-model"
ensure_worktree r26-projection-scroll "$BASE/r26-projection-scroll"
ensure_worktree r27-input-modes-mouse "$BASE/r27-input-modes"
ensure_worktree r28-streaming-scheduler "$BASE/r28-streaming"
ensure_worktree r29-bb-integration "$BASE/r29-integration"

launch_window foundation  "$BASE/r24-foundation"        "$TASKS/r24-fullscreen-foundation.md" "$LOG_DIR/r24.log"
launch_window blocks      "$BASE/r25-block-model"       "$TASKS/r25-transcript-block-model.md" "$LOG_DIR/r25.log"
launch_window projection  "$BASE/r26-projection-scroll" "$TASKS/r26-projection-scroll.md" "$LOG_DIR/r26.log"
launch_window input       "$BASE/r27-input-modes"       "$TASKS/r27-input-modes-mouse.md" "$LOG_DIR/r27.log"
launch_window streaming   "$BASE/r28-streaming"         "$TASKS/r28-streaming-scheduler.md" "$LOG_DIR/r28.log"
launch_window integrate   "$BASE/r29-integration"       "$TASKS/r29-bb-integration.md" "$LOG_DIR/r29.log"

printf 'launched tmux session: %s (6 windows)\n' "$SESSION"
printf 'attach: tmux attach -t %s\n' "$SESSION"
