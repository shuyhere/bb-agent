#!/usr/bin/env bash
set -euo pipefail

BASE_COMMIT="6652737"
SESSION="bb-fullscreen-next"
OUT="/tmp/bb-fullscreen-next/monitor.log"
INTERVAL="${1:-45}"

BRANCHES=(
  "/tmp/bb-fullscreen-next/r30-unify"
  "/tmp/bb-fullscreen-next/r31-projector"
  "/tmp/bb-fullscreen-next/r32-controls"
  "/tmp/bb-fullscreen-next/r33-streaming"
  "/tmp/bb-fullscreen-next/r34-runtime"
)

summarize_branch() {
  local dir="$1"
  local branch head commits dirty shortstat changed_files
  branch="$(git -C "$dir" branch --show-current 2>/dev/null || echo unknown)"
  head="$(git -C "$dir" rev-parse --short HEAD 2>/dev/null || echo none)"
  commits="$(git -C "$dir" log --oneline ${BASE_COMMIT}..HEAD 2>/dev/null | wc -l | tr -d ' ')"
  shortstat="$(git -C "$dir" diff --shortstat 2>/dev/null || true)"
  dirty="$(git -C "$dir" status --porcelain 2>/dev/null | wc -l | tr -d ' ')"
  changed_files="$(git -C "$dir" diff --name-only 2>/dev/null | wc -l | tr -d ' ')"

  echo "[$branch] head=$head new_commits=$commits dirty_entries=$dirty changed_files=$changed_files"
  if [ -n "$shortstat" ]; then
    echo "  diff: $shortstat"
  else
    echo "  diff: clean"
  fi

  if [ "$commits" -gt 0 ]; then
    echo "  recent commits:"
    git -C "$dir" log --oneline --decorate -n 3 ${BASE_COMMIT}..HEAD 2>/dev/null | sed 's/^/    /'
  fi

  if [ "$changed_files" -gt 0 ]; then
    echo "  changed paths:"
    git -C "$dir" diff --name-only 2>/dev/null | sed -n '1,12p' | sed 's/^/    /'
  fi

  if [ "$changed_files" -gt 25 ]; then
    echo "  review-warning: diff is broad; verify branch stayed in scope"
  fi
}

mkdir -p /tmp/bb-fullscreen-next

while true; do
  {
    echo "============================================================"
    date --iso-8601=seconds
    if tmux has-session -t "$SESSION" 2>/dev/null; then
      echo "tmux: $SESSION active"
      tmux list-windows -t "$SESSION" | sed 's/^/  /'
    else
      echo "tmux: $SESSION missing"
    fi
    for dir in "${BRANCHES[@]}"; do
      summarize_branch "$dir"
    done
    echo
  } | tee -a "$OUT"
  sleep "$INTERVAL"
done
