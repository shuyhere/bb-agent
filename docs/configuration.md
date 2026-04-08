# Configuration

BB-Agent uses a layered configuration system. Project settings override global settings.

## File Locations

| File | Purpose |
|------|---------|
| `~/.bb-agent/settings.json` | Global settings |
| `<project>/.bb-agent/settings.json` | Project-local settings |
| `~/.bb-agent/AGENTS.md` | Global system prompt additions |
| `<project>/AGENTS.md` | Project system prompt additions |
| `~/.bb-agent/auth.json` | Stored API keys and OAuth tokens |
| `~/.bb-agent/sessions.db` | Session database |
| `~/.bb-agent/system-prompts/` | Named system prompt templates |
| `~/.bb-agent/skills/` | Global skills |
| `~/.bb-agent/extensions/` | Global extensions |
| `~/.bb-agent/prompts/` | Global prompt templates |

Project root is detected by walking up from `cwd` looking for `.git`, `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`, `.hg`, `AGENTS.md`, or `CLAUDE.md`.

## settings.json Reference

```json
{
  "execution_mode": "safety",
  "default_provider": "anthropic",
  "default_model": "claude-sonnet-4-20250514",
  "default_thinking": "medium",
  "execution_mode": "safety",

  "compaction": {
    "enabled": true,
    "reserve_tokens": 16384,
    "keep_recent_tokens": 20000
  },

  "retry": {
    "enabled": true,
    "max_retries": 3,
    "base_delay_ms": 2000,
    "max_delay_ms": 60000
  },

  "tools": null,
  "extensions": [],
  "skills": [],
  "prompts": [],
  "packages": [],

  "enable_skill_commands": true,

  "models": [
    {
      "id": "my-local-model",
      "name": "My Local Model",
      "provider": "ollama",
      "api": "openai",
      "base_url": "http://localhost:11434/v1",
      "context_window": 32000,
      "max_tokens": 4096,
      "reasoning": false
    }
  ],

  "providers": [
    {
      "name": "custom",
      "base_url": "https://my-api.example.com/v1",
      "api_key_env": "MY_API_KEY",
      "api": "openai",
      "headers": {
        "X-Custom-Header": "value"
      }
    }
  ],

  "color_theme": "lavender",
  "compatibility_mode": false,

  "update_check": {
    "enabled": true,
    "ttl_hours": 24
  }
}
```

### Fields

#### `execution_mode`
Execution posture for built-in tools.

- `safety` (default)
  - restricts built-in `write` and `edit` to files inside the active workspace
  - runs `bash` in the safer approval/sandboxed posture
- `yolo`
  - allows broader built-in file mutation behavior
  - skips the safer bash posture

BB-Agent shows the active posture in `/session` and the fullscreen footer/settings UI so it stays visible during a run.

#### `default_provider`
Default LLM provider. Values: `anthropic`, `openai`, `google`, `groq`, `xai`, `openrouter`, or a custom provider name.

#### `default_model`
Default model ID. Can also be set via `--model` CLI flag.

#### `default_thinking`
Default thinking level: `off`, `low`, `medium`, `high`. Controls extended thinking for supported models.

#### `compaction`
Automatic context compaction when approaching the model's context window limit.
- `enabled` — enable/disable compaction
- `reserve_tokens` — tokens to keep free for the next response
- `keep_recent_tokens` — always keep this many tokens of recent context

#### `retry`
Auto-retry on transient provider errors (429, 500, 502, 503, etc.).
- `max_retries` — maximum retry attempts
- `base_delay_ms` — initial backoff delay
- `max_delay_ms` — maximum backoff delay (also caps server-requested delays)

#### `tools`
Restrict which built-in tools are available. `null` means all tools enabled. Example: `["read", "bash", "edit", "write"]`

#### `extensions`
Paths to JS/TS extension files or directories. Loaded at startup.

#### `skills`
Additional paths to scan for skill files.

#### `prompts`
Additional paths to scan for prompt template files.

#### `packages`
Installed package sources. Managed via `bb install`, `bb remove`, `bb update`.

#### `models`
Custom model definitions. Fields:
- `id` (required) — model identifier
- `provider` (required) — which provider handles this model
- `api` — API type: `openai`, `anthropic`, `google`
- `base_url` — custom API endpoint
- `context_window` — context window size in tokens
- `max_tokens` — max output tokens
- `reasoning` — whether model supports extended thinking

#### `providers`
Custom provider overrides. Fields:
- `name` (required) — provider name
- `base_url` — API endpoint URL
- `api_key_env` — environment variable name for the API key
- `api` — API type
- `headers` — additional HTTP headers

#### `color_theme`
TUI color theme. Currently supported: `lavender` (default), or custom.

#### `compatibility_mode`
Enable ASCII-safe fallback rendering for terminals/fonts that do not display BB-Agent's richer Unicode glyphs correctly.

When enabled, BB-Agent uses safer fallback symbols for spinner frames, live tool markers, and some transcript decorations.

Equivalent environment variable:

```bash
BB_TUI_COMPAT=1
```

## Migration Notes

- `execution_mode` now defaults to `yolo`.
- Use `"execution_mode": "safety"` if you want built-in `write` or `edit` restricted to the current project directory.
- In `safety` mode, non-read-only bash commands now go through the safer approval/sandboxed path instead of running freely.

## AGENTS.md

`AGENTS.md` (or `CLAUDE.md` as fallback) files are appended to the system prompt. BB-Agent loads them from multiple levels and merges them:

1. `~/.bb-agent/AGENTS.md` — global rules
2. From project root down to cwd, each `AGENTS.md` found — project rules

Files are joined with `---` separators, global first.

### Example

```markdown
# Project Rules
- This is a Rust project using the 2024 edition
- Always run `cargo test` after changes
- Prefer explicit error handling over unwrap()
```

## System Prompt Templates

Save `.md` files in `~/.bb-agent/system-prompts/`:

```
~/.bb-agent/system-prompts/
├── coding.md
├── research.md
└── review.md
```

Use with:
```bash
bb -t coding       # Use the "coding" template
bb --list-templates # List all available templates
```

Templates fully replace the default system prompt when used.

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `OPENAI_API_KEY` | OpenAI API key |
| `GOOGLE_API_KEY` or `GEMINI_API_KEY` | Google AI API key |
| `GROQ_API_KEY` | Groq API key |
| `XAI_API_KEY` | xAI API key |
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `BB_BROWSER` | Path to Chrome/Chromium binary for `browser_fetch` |
| `BB_TUI_COMPAT` | Enable ASCII-safe TUI compatibility mode |
