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
  local branch="$1"
  local dir="$2"
  if [ -e "$dir" ]; then
    echo "reusing worktree: $dir"
    return
  fi

  if git -C "$ROOT" show-ref --verify --quiet "refs/heads/$branch"; then
    git -C "$ROOT" worktree add "$dir" "$branch"
  else
    git -C "$ROOT" worktree add -b "$branch" "$dir" master
  fi
}

launch_window() {
  local window="$1"
  local dir="$2"
  local task="$3"
  local log="$4"
  local cmd="cd '$dir' && printf '\n[%s] worktree: %s\n' '$window' '$dir' && PROMPT=\"\$(cat '$task')\" && pi --mode json -p --no-session \"\$PROMPT\" 2>&1 | tee '$log'; printf '\n[%s] finished. log: %s\n' '$window' '$log'; exec bash"

  if tmux has-session -t "$SESSION" 2>/dev/null; then
    tmux new-window -t "$SESSION" -n "$window" "$cmd"
  else
    tmux new-session -d -s "$SESSION" -n "$window" "$cmd"
    tmux set-option -t "$SESSION" remain-on-exit on >/dev/null
  fi
}

ensure_worktree "r01-split-agent" "$BASE/r01-agent"
ensure_worktree "r02-split-agent-session" "$BASE/r02-agent-session"
ensure_worktree "r03-split-editor-pass1" "$BASE/r03-editor-pass1"

if tmux has-session -t "$SESSION" 2>/dev/null; then
  tmux kill-session -t "$SESSION"
fi

launch_window "agent" "$BASE/r01-agent" "$ROOT/.bb-agent/tasks/restructure/r01-split-agent.md" "$LOG_DIR/r01-agent.log"
launch_window "agent-session" "$BASE/r02-agent-session" "$ROOT/.bb-agent/tasks/restructure/r02-split-agent-session.md" "$LOG_DIR/r02-agent-session.log"
launch_window "editor-pass1" "$BASE/r03-editor-pass1" "$ROOT/.bb-agent/tasks/restructure/r03-split-editor-pass1.md" "$LOG_DIR/r03-editor-pass1.log"

printf 'launched tmux session: %s\n' "$SESSION"
printf 'attach: tmux attach -t %s\n' "$SESSION"
printf 'logs: %s\n' "$LOG_DIR"
