#!/usr/bin/env bash
set -euo pipefail

BASE_COMMIT="3a97a78"
STATE_DIR="/tmp/bb-fullscreen/review-state"
LOG="/tmp/bb-fullscreen/review-events.log"
INTERVAL="${1:-30}"

mkdir -p "$STATE_DIR"

BRANCH_DIRS=(
  "/tmp/bb-fullscreen/r24-foundation"
  "/tmp/bb-fullscreen/r25-block-model"
  "/tmp/bb-fullscreen/r26-projection-scroll"
  "/tmp/bb-fullscreen/r27-input-modes"
  "/tmp/bb-fullscreen/r28-streaming"
  "/tmp/bb-fullscreen/r29-integration"
)

while true; do
  for dir in "${BRANCH_DIRS[@]}"; do
    branch="$(git -C "$dir" branch --show-current 2>/dev/null || basename "$dir")"
    head="$(git -C "$dir" rev-parse HEAD 2>/dev/null || echo none)"
    state_file="$STATE_DIR/$branch.head"
    old_head=""
    [ -f "$state_file" ] && old_head="$(cat "$state_file")"

    if [ "$head" != "$old_head" ]; then
      if [ -z "$old_head" ] && [ "$head" = "$BASE_COMMIT" ]; then
        printf '%s' "$head" > "$state_file"
        continue
      fi
      {
        echo "============================================================"
        date --iso-8601=seconds
        echo "branch: $branch"
        echo "old_head: ${old_head:-<none>}"
        echo "new_head: $head"
        echo "new commits since previous head:"
        if [ -n "$old_head" ]; then
          git -C "$dir" log --oneline --decorate "$old_head".."$head" || true
        else
          git -C "$dir" log --oneline --decorate ${BASE_COMMIT}.."$head" || true
        fi
        echo "head stat:"
        git -C "$dir" show --stat --summary --oneline --no-renames -n 1 "$head" || true
        echo
      } | tee -a "$LOG"
      printf '%s' "$head" > "$state_file"
    fi
  done
  sleep "$INTERVAL"
done
