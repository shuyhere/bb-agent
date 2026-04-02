#!/usr/bin/env bash
set -euo pipefail

ROOT="/home/shuyhere/BB-Agent"
BASE="/tmp/bb-restructure"
LOG_DIR="$BASE/logs"
SESSION="bb-restructure"

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
  local cmd="cd '$dir' && PROMPT=\"\$(cat '$task')\" && pi -p --no-session \"\$PROMPT\" 2>&1 | tee '$log'; printf '\n[%s] done. log: %s\n' '$window' '$log'; exec bash"
  if tmux has-session -t "$SESSION" 2>/dev/null; then
    tmux new-window -t "$SESSION" -n "$window" "$cmd"
  else
    tmux new-session -d -s "$SESSION" -n "$window" "$cmd"
  fi
}

# Kill old session if exists
tmux kill-session -t "$SESSION" 2>/dev/null || true

# Create worktrees
ensure_worktree r04-split-extensions       "$BASE/r04-extensions"
ensure_worktree r05-interactive-mode-decompose "$BASE/r05-interactive-mode"
ensure_worktree r06-split-agent-loop       "$BASE/r06-agent-loop"
ensure_worktree r07-split-editor-input     "$BASE/r07-editor-input"
ensure_worktree r08-split-core-types       "$BASE/r08-core-types"
ensure_worktree r09-fix-pub-use-star       "$BASE/r09-pub-use-star"

# Launch windows
launch_window extensions       "$BASE/r04-extensions"       "$ROOT/.bb-agent/tasks/restructure/r04-split-extensions.md"          "$LOG_DIR/r04.log"
launch_window interactive-mode "$BASE/r05-interactive-mode"  "$ROOT/.bb-agent/tasks/restructure/r05-interactive-mode-decompose.md" "$LOG_DIR/r05.log"
launch_window agent-loop       "$BASE/r06-agent-loop"       "$ROOT/.bb-agent/tasks/restructure/r06-split-agent-loop.md"           "$LOG_DIR/r06.log"
launch_window editor-input     "$BASE/r07-editor-input"     "$ROOT/.bb-agent/tasks/restructure/r07-split-editor-input.md"         "$LOG_DIR/r07.log"
launch_window core-types       "$BASE/r08-core-types"       "$ROOT/.bb-agent/tasks/restructure/r08-split-core-types.md"           "$LOG_DIR/r08.log"
launch_window pub-use-star     "$BASE/r09-pub-use-star"     "$ROOT/.bb-agent/tasks/restructure/r09-fix-pub-use-star.md"           "$LOG_DIR/r09.log"

printf 'launched tmux session: %s (6 windows)\n' "$SESSION"
printf 'attach: tmux attach -t %s\n' "$SESSION"
printf 'logs: %s/r0{4..9}.log\n' "$LOG_DIR"
