# Task: split `crates/core/src/agent_session_extensions.rs`

Worktree: `/tmp/bb-restructure/r04-extensions`
Branch: `r04-split-extensions`

## Goal
This is the worst god file in the repo: 784 lines, 43 structs/enums/impls, mixing types + IO + orchestration + utilities + discovery in one flat file.

Split it into a module tree at `crates/core/src/agent_session_extensions/`.

## Principles to follow
- `mod.rs` declares modules and re-exports only. Zero logic.
- One file, one responsibility.
- Separate types from behavior.
- Keep public facade small and intentional.

## Likely split
Read the file carefully and split by responsibility boundary:

1. `types.rs` — all the public structs/enums that are pure data (UiContextBinding, CommandContextAction, ExtensionBindings, DiscoveredResourcePath, ResourcesDiscoverResult, ResourcePathMetadata, ResourceScope, ResourceOrigin, ResourcePathEntry, ResourceExtensionPaths, SourceInfo, SlashCommandInfo, SlashCommandSource, PromptTemplateInfo, SkillInfo, SkillCatalog, RegisteredCommand, ToolDefinition, AgentTool, ToolDefinitionEntry, RegisteredTool, RuntimeFlagValue, ExtensionRuntimeState, LoadedExtension, ExtensionsResult, SessionSettings, RefreshToolRegistryOptions, RuntimeBuildOptions, ModelDescriptor, ProviderConfig)
2. `resources.rs` — ResourceLoaderState struct and its impl
3. `models.rs` — ModelRegistryState struct and its impl  
4. `runner.rs` — ExtensionRunnerState struct and its impl (the largest impl block with orchestration logic)
5. `mod.rs` — routing + re-exports only

## Constraints
- Do NOT redesign behavior.
- Do NOT rename types.
- Do NOT change public API surface.
- Touch other files ONLY if needed for imports.
- Use `pub(super)` for internal-only items.

## Verification
```
cargo build -q
cargo test -q -p bb-core
```

## Finish
```
git add -A
git commit -m "split agent_session_extensions into module tree by responsibility"
```

Report: changed files, verification results, commit hash.
