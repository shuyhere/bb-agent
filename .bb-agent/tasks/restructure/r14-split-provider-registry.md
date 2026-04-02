# Task: split `crates/provider/src/registry.rs` (514 lines)

Worktree: `/tmp/bb-restructure/r14-provider-registry`
Branch: `r14-split-provider-registry`

## Goal
`registry.rs` mixes model data definitions with registry lookup logic and API type mapping.

Split by responsibility.

## Likely split
Read the file and separate:

1. `registry/types.rs` — Model struct, ApiType enum, provider metadata types
2. `registry/models.rs` — the large static model data (all the model definitions/entries)
3. `registry/lookup.rs` — ModelRegistry struct and its lookup/query methods
4. `registry/mod.rs` — routing + re-exports

Or if a module tree feels too heavy, at minimum split:
- `registry.rs` — keeps ModelRegistry + lookup logic
- `models_data.rs` — the static model definitions

## Constraints
- Do NOT change behavior.
- Preserve `bb_provider::registry::*` public surface.
- Keep `mod.rs` routing only if you create a directory.

## Verification
```
cargo build -q -p bb-provider
```

## Finish
```
git add -A && git commit -m "split provider registry by responsibility"
```
