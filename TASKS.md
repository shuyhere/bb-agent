# Remaining TUI Tasks

## Task 1: Fix Editor + Interactive Flow
- Editor should emit submit events cleanly via a channel
- Ctrl+C: cancel agent or clear editor (double Ctrl+C exits)
- Ctrl+D: exit when editor empty
- Escape: interrupt running agent
- Fix streaming text: assistant header should show before text starts
- Wire up agent cancellation properly

## Task 2: Streaming Markdown
- During streaming, show raw text
- On completion, replace with markdown-rendered version
- Handle incremental rendering (don't re-render entire chat on each delta)

## Task 3: Footer Improvements
- Detect git branch via `git rev-parse --abbrev-ref HEAD`
- Update on each render cycle (cached, re-checked periodically)
- Show session name if set

## Task 4: Clean Compilation
- Fix all warnings
- Remove unused imports
- Ensure cargo clippy passes

## Task 5: Visual Testing
- Run `pi` and `bb` side by side in tmux
- Compare: header, editor border, footer layout, streaming output
- Screenshot comparison
