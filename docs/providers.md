# Providers & Models

BB-Agent supports multiple LLM providers out of the box.

## Supported Providers

| Provider | Auth Method | Models |
|----------|-------------|--------|
| **Anthropic** | OAuth or `ANTHROPIC_API_KEY` | Claude Opus, Sonnet, Haiku |
| **OpenAI** | OAuth or `OPENAI_API_KEY` | GPT-4o, GPT-4.1, o1, o3, o4-mini |
| **GitHub Copilot** | OAuth/device-flow skeleton + stored authority | Runtime skeleton |
| **Google** | `GOOGLE_API_KEY` | Gemini 2.5 Pro, Flash |
| **Groq** | `GROQ_API_KEY` | Llama, Mixtral |
| **xAI** | `XAI_API_KEY` | Grok |
| **OpenRouter** | `OPENROUTER_API_KEY` | 100+ models |
| **Custom** | Configurable | Any OpenAI-compatible API |

## Authentication

### OAuth Login (Anthropic, OpenAI, GitHub Copilot preview)

```bash
bb login anthropic        # Opens browser for OAuth
bb login openai-codex     # Opens browser for OAuth
bb login github-copilot   # Stores github.com or GHES host for upcoming OAuth flow
```

For GitHub Copilot, `bb` currently supports:
- stored authority-aware configuration (`github.com` or GitHub Enterprise Server domain)
- fullscreen/CLI browser + device-flow auth scaffolding
- token storage/refresh plumbing skeleton
- runtime/model skeleton under the `github-copilot` provider

Still not implemented yet:
- real Copilot token exchange
- real Copilot token refresh
- verified production request/streaming support

### API Key Login

```bash
bb login google         # Prompts for API key
bb login groq
bb login xai
bb login openrouter
```

### Environment Variables

Set directly without `bb login`:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
export OPENAI_API_KEY="sk-..."
export GOOGLE_API_KEY="..."
export GROQ_API_KEY="..."
export XAI_API_KEY="..."
export OPENROUTER_API_KEY="..."
```

### Check Status

```bash
bb login    # Shows ✓/✗ for each provider
```

## Selecting a Model

### CLI Flags

```bash
bb --model sonnet                                # Fuzzy match
bb --model claude-sonnet-4-20250514              # Exact model ID
bb --model anthropic/claude-sonnet-4-20250514    # Provider/model
bb --model sonnet:high                           # Model with thinking level
bb --provider google --model gemini-2.5-flash    # Explicit provider
```

### Thinking Levels

For models that support extended thinking:

```bash
bb --model sonnet:high      # High thinking budget
bb --model sonnet:medium    # Medium (default)
bb --model sonnet:low       # Low
bb --model sonnet:off       # No extended thinking
bb --thinking high          # Set thinking separately
```

### List Available Models

```bash
bb --list-models            # List all models
bb --list-models sonnet     # Search/filter
bb --list-models groq       # Models from a provider
```

### In-Session Model Switching

Press `Ctrl+P` to cycle through models, or use:
```
/model sonnet
/model gpt-4o
/model openai/gpt-4o
/model openai:gpt-4o
/model sonnet:high
/model anthropic/claude-sonnet-4-20250514:low
```

`/model` now accepts common provider/model and thinking-suffix formats directly during a conversation.

### Default Model

In `settings.json`:
```json
{
  "default_provider": "anthropic",
  "default_model": "claude-sonnet-4-20250514",
  "default_thinking": "medium"
}
```

## Custom Models

Add models that aren't in the built-in registry:

```json
{
  "models": [
    {
      "id": "llama3-70b",
      "name": "Llama 3 70B (local)",
      "provider": "ollama",
      "api": "openai",
      "base_url": "http://localhost:11434/v1",
      "context_window": 8192,
      "max_tokens": 4096,
      "reasoning": false
    }
  ]
}
```

## Custom Providers

Define entirely new providers:

```json
{
  "providers": [
    {
      "name": "my-corp",
      "base_url": "https://llm.internal.corp.com/v1",
      "api_key_env": "CORP_LLM_KEY",
      "api": "openai",
      "headers": {
        "X-Team": "engineering"
      }
    }
  ]
}
```

Then use:
```bash
bb --provider my-corp --model our-model
```

## API Types

The `api` field determines the request/response format:

| Value | Compatible With |
|-------|----------------|
| `openai` | OpenAI, Groq, xAI, OpenRouter, Ollama, vLLM, LiteLLM |
| `anthropic` | Anthropic |
| `google` | Google Gemini |
