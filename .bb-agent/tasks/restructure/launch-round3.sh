#!/usr/bin/env bash
set -euo pipefail

ROOT="/home/shuyhere/BB-Agent"
BASE="/tmp/bb-restructure"
LOG_DIR="$BASE/logs"
SESSION="bb-restructure"
TASKS="$ROOT/.bb-agent/tasks/restructure"

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

ensure_worktree r10-fix-remaining-pub-use-star "$BASE/r10-pub-star"
ensure_worktree r11-add-error-types            "$BASE/r11-errors"
ensure_worktree r12-move-io-out-of-core        "$BASE/r12-core-io"
ensure_worktree r13-split-plugin-host          "$BASE/r13-plugin-host"
ensure_worktree r14-split-provider-registry    "$BASE/r14-provider-registry"
ensure_worktree r15-clean-lib-rs               "$BASE/r15-lib-rs"

launch_window pub-star       "$BASE/r10-pub-star"          "$TASKS/r10-fix-remaining-pub-use-star.md"  "$LOG_DIR/r10.log"
launch_window errors         "$BASE/r11-errors"            "$TASKS/r11-add-error-types.md"             "$LOG_DIR/r11.log"
launch_window core-io        "$BASE/r12-core-io"           "$TASKS/r12-move-io-out-of-core.md"         "$LOG_DIR/r12.log"
launch_window plugin-host    "$BASE/r13-plugin-host"       "$TASKS/r13-split-plugin-host.md"           "$LOG_DIR/r13.log"
launch_window registry       "$BASE/r14-provider-registry" "$TASKS/r14-split-provider-registry.md"     "$LOG_DIR/r14.log"
launch_window lib-rs         "$BASE/r15-lib-rs"            "$TASKS/r15-clean-lib-rs.md"                "$LOG_DIR/r15.log"

printf 'launched tmux session: %s (6 windows)\n' "$SESSION"
printf 'attach: tmux attach -t %s\n' "$SESSION"
