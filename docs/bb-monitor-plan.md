# bb-monitor extraction plan

## Goal

Create a new backend-focused crate, `bb-monitor`, that centralizes BB-Agent monitoring and metric logic without moving TUI rendering itself.

This crate should own the data model and pure helpers for:
- token / cost / cache usage summaries
- context-window status and formatting
- KV-cache Phase 1 request metrics
  - cache-hit metrics
  - prefix divergence
  - warm/cold request classification
  - latency / TTFT / resume-latency accounting
- provider cache-signal attribution (`official` vs `estimated` vs `unknown`)

The TUI should keep rendering, but it should stop owning monitor math/string logic directly.

## Current audit: where monitor logic lives today

### 1. Provider usage producers
Current producer paths on `master`:
- `crates/provider/src/anthropic/events.rs`
- `crates/provider/src/google/events.rs`
- `crates/provider/src/openai/sse.rs`
- `crates/provider/src/openai/codex.rs`
- `crates/provider/src/streaming.rs`

What they already produce:
- `input_tokens`
- `output_tokens`
- `cache_read_tokens`
- `cache_write_tokens`

What is still missing on clean `master`:
- explicit cache-metric source provenance (`official` / `estimated` / `unknown`)

### 2. Persisted usage + cost model
Current persisted usage/cost types:
- `crates/core/src/types/messages.rs`
  - `Cost`
  - `Usage`

Related legacy duplicate shape still exists in:
- `crates/core/src/agent/data.rs`
- `crates/core/src/agent_loop/types.rs`

This is one reason the agent/runtime layers still feel noisy: monitor/state vocabulary is duplicated across core and legacy loop types.

### 3. Session-wide summaries
Current aggregation lives in:
- `crates/cli/src/session_info.rs`

That file currently owns backend logic for:
- summing input/output/cache read/cache write tokens
- summing total cost
- formatting large counts for human-readable display
- deciding which cache/cost fields appear in `/session` output

This belongs in `bb-monitor`, not in CLI.

### 4. Context-window / footer monitor logic
Current backend-ish logic is mixed into UI controller code:
- `crates/cli/src/tui/controller/ui.rs`

That file currently owns:
- footer usage totals lookup
- context-window estimation fallback rules
- `?/272k (auto)` vs `75.9%/272k (auto)` formatting
- compact usage-text assembly like:
  - `↑13M ↓754k R275M $112.751 (sub) 75.9%/272k (auto)`

Even without changing TUI widgets, these pure functions should move into `bb-monitor`.

### 5. Existing KV-cache Phase 1 prototype
Your strongest prototype already exists in:
- `/home/shuyhere/bb-cache-metrics-clean/crates/cli/src/cache_metrics.rs`

It already defines a useful Phase 1 model:
- request hash / stable-prefix hash
- previous request hash
- first divergence byte / token estimate
- reused prefix byte / token estimate
- prompt bytes / message count / tool count
- cache metrics source
- provider vs estimated cache read/write tokens
- warm request classification
- TTFT / total latency / tool wait / resume latency
- request mutation flags
- JSONL logging of request metrics

This prototype is the correct source material for `bb-monitor`.

## What `bb-monitor` should own

### Phase 0: crate scaffold (started)
- new crate: `crates/bb-monitor`
- pure backend modules:
  - `usage`
  - `cache_metrics`
- no TUI rendering dependency

### Phase 1: pure monitor vocabulary
Move or define in `bb-monitor`:
- `UsageTotals`
- `ContextWindowStatus`
- compact token formatting
- context usage formatting
- footer-usage text assembly
- cache rate helpers:
  - `cache_read_hit_rate_pct`
  - `cache_effective_utilization_pct`
- `CacheMetricsSource`
- request metric structs:
  - `PreparedRequestMetrics`
  - `ResolvedCacheUsage`
  - `RequestCacheMetrics`
  - `RequestMutationFlags`

### Phase 2: backend aggregation extraction
Move pure aggregation out of CLI files into `bb-monitor`:
- from `crates/cli/src/session_info.rs`
  - usage/cost/cache totals aggregation
  - cache-metric source rollups
  - numeric/text formatting helpers
- from `crates/cli/src/tui/controller/ui.rs`
  - footer-usage text assembly helpers
  - context-window footer formatting helpers

Important: keep TUI command/rendering code in CLI/TUI crates; only move monitor math/text construction.

### Phase 3: provider + persistence wiring
Port the useful pieces from `bb-cache-metrics-clean` into `bb-monitor` and then wire them through:
- `bb-provider`
  - add cache-metric source provenance to usage events
- `bb-core`
  - extend persisted `Usage` with `cache_metrics_source`
- `bb-cli`
  - request preparation/finalization hooks call `bb-monitor`
  - request metrics logger writes JSONL via `bb-monitor`

### Phase 4: legacy cleanup
Once the new path is live:
- reduce legacy duplicates in `crates/core/src/agent/data.rs`
- reduce legacy monitor/context shapes in `crates/core/src/agent_loop/types.rs`
- make `bb-monitor` the canonical backend monitor vocabulary

## Proposed implementation order

1. **Scaffold `bb-monitor`** with shared pure helpers and tests. ✅ started here
2. Migrate session summary aggregation from `crates/cli/src/session_info.rs`
3. Migrate footer usage/context text helpers from `crates/cli/src/tui/controller/ui.rs`
4. Introduce `CacheMetricsSource` into persisted usage types
5. Move/port the `bb-cache-metrics-clean` request-metrics implementation into `bb-monitor`
6. Wire request metrics into the turn runner and provider event collection
7. Only after backend is stable, decide whether TUI-specific render code should call more `bb-monitor` helpers directly

## Non-goals for this pass

- no TUI widget/layout changes
- no redesign of footer visuals
- no frontend-specific color/styling logic in `bb-monitor`

## Success criteria

- backend monitor/state vocabulary is no longer scattered across CLI/TUI/controller files
- `bb-monitor` owns the reusable token/cost/cache/context helpers
- KV-cache Phase 1 request metrics live in a crate that can be reused by CLI, session summaries, and future non-TUI reporting paths
- core agent loop code gets clearer because monitor bookkeeping moves out into a dedicated crate
