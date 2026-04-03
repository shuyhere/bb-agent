# R51: Package Auto-Install on Startup

## Goal
When BB-Agent starts in interactive or print mode, check if project settings
contain packages whose install directories don't exist, and auto-install them.
This matches pi behavior: "pi installs any missing packages automatically on
startup."

## Scope
- Add `auto_install_missing_packages(cwd, settings)` in `crates/cli/src/extensions.rs`.
- For each `PackageEntry` in `settings.packages`, resolve the expected install
  directory. If it doesn't exist and the source is `npm:` or `git:`, run install.
- Call this function early in `crates/cli/src/run.rs` (print mode) and
  `crates/cli/src/interactive.rs` (interactive mode) before loading extensions.
- Log what was installed via `tracing::info!`.

## Files to Touch
- `crates/cli/src/extensions.rs` — add `auto_install_missing_packages()`.
- `crates/cli/src/run.rs` — call it before `load_runtime_extension_support`.
- `crates/cli/src/interactive.rs` — call it before setup.

## Constraints
- Keep changes in `crates/cli/` only.
- Must compile with `cargo build -q -p bb-cli`.
- Must pass `cargo test -q -p bb-cli`.
- Add a test that verifies the function identifies a missing package dir
  and would install it (mock or use a local path package that doesn't need
  real npm/git).
