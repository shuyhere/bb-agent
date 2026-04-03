# R52: Glob Pattern Support in Package Filters

## Goal
Upgrade package filter matching from substring/suffix matching to real glob
patterns so that filters like `["extensions/*.ts", "!extensions/legacy.ts"]`
work correctly per pi docs.

## Scope
- Add `glob = "0.3"` to `crates/cli/Cargo.toml`.
- Rewrite `filter_matches()` in `crates/cli/src/extensions.rs` to:
  - Use glob pattern matching for include patterns.
  - Use glob pattern matching for `!pattern` exclusions.
  - Keep `+path` / `-path` as exact-match force include/exclude.
  - Match relative paths from the package root.
- Update existing tests and add new ones for glob patterns:
  - `extensions/*.ts` matches `extensions/foo.ts` but not `extensions/sub/bar.ts`
  - `extensions/**/*.ts` matches nested files
  - `!extensions/legacy.ts` excludes exact match

## Files to Touch
- `crates/cli/Cargo.toml` — add `glob` dependency.
- `crates/cli/src/extensions.rs` — rewrite `filter_matches()`, update tests.

## Constraints
- Keep changes in `crates/cli/` only.
- Must compile with `cargo build -q -p bb-cli`.
- Must pass `cargo test -q -p bb-cli`.
