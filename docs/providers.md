# Providers & Models

BB-Agent supports multiple LLM providers out of the box.

## Supported Providers

| Provider | Auth Method | Models |
|----------|-------------|--------|
| **Anthropic** | OAuth or `ANTHROPIC_API_KEY` | Claude 3.x, Claude 4.x, latest aliases |
| **OpenAI** | OAuth or `OPENAI_API_KEY` | GPT-4.x, GPT-4o, GPT-5.x, o-series |
| **GitHub Copilot** | OAuth/device flow or `GH_COPILOT_TOKEN` | Claude 4.x, GPT-4.1/4o/5.x, Gemini previews, Grok Code |
| **Google** | `GOOGLE_API_KEY` | Gemini 1.5/2.x/3.x, Gemma 3/4 |
| **Groq** | `GROQ_API_KEY` | Llama 3/4, Kimi K2, GPT-OSS, Qwen, Compound |
| **xAI** | SuperGrok / Premium+ OAuth or `XAI_API_KEY` | Grok (builtin registry + OpenAI-compatible API) |
| **OpenRouter** | `OPENROUTER_API_KEY` | Curated Claude, Gemini, GPT-5, DeepSeek models |
| **Custom** | Configurable | Any OpenAI-compatible API |

## Authentication

### OAuth Login (Anthropic, OpenAI, GitHub Copilot, xAI)

```bash
bb login anthropic        # Opens browser for OAuth
bb login openai-codex     # Opens browser for OAuth
bb login github-copilot   # GitHub device flow + Copilot token exchange
bb login xai              # Choose SuperGrok OAuth or API key
```

For GitHub Copilot, `bb` now supports:
- stored authority-aware configuration (`github.com` or GitHub Enterprise Server domain)
- GitHub device/browser auth flow
- GitHub OAuth token persistence in `auth.json`
- Copilot runtime token exchange via GitHub's Copilot token endpoint
- Copilot runtime token refresh by re-exchanging the saved GitHub OAuth session when `GITHUB_COPILOT_CLIENT_SECRET` (or `GH_COPILOT_CLIENT_SECRET`) is provided
- `/models` validation and cached Copilot model discovery
- Copilot auth/session visibility in `/session`

Current limitations:
- Copilot request behavior is wired through the OpenAI-compatible runtime path and may still need endpoint/header adjustments for some models or enterprise installations
- Enterprise endpoint behavior still needs more real-world validation

#### xAI SuperGrok / X Premium+ OAuth

xAI dual auth works like Anthropic: OAuth and API key can coexist for provider `xai`.

```bash
bb login xai              # Prompt: 1) SuperGrok OAuth  2) API key
bb --model xai/grok-build-0.1 -p "hello"
```

Details:
- Device-code login against `auth.x.ai` / `accounts.x.ai` (no loopback callback required)
- Uses xAI's **public Grok CLI OIDC client** (no client secret; same ecosystem as Grok CLI / Hermes SuperGrok OAuth)
- Tokens stored under the `xai-oauth` auth profile; API keys remain under `xai`
- Inference uses OpenAI-compatible `https://api.x.ai/v1` with the OAuth access token as Bearer
- Automatic refresh when the access token is near expiry

Limitations:
- Requires SuperGrok or X Premium+ entitlement for OAuth API access
- xAI may return **HTTP 403** for some accounts even when the web subscription is active (tier / allowlist). In that case use `XAI_API_KEY` instead
- OAuth is subscription-session access, not Console pay-as-you-go billing

### API Key Login

```bash
bb login google         # Prompts for API key
bb login groq
bb login xai            # Choose method 2 for API key
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
export GH_COPILOT_TOKEN="..."                 # Direct Copilot runtime token
export GITHUB_COPILOT_TOKEN="..."             # Equivalent env fallback
export GITHUB_COPILOT_CLIENT_SECRET="..."     # Optional: only needed for GitHub OAuth refresh support
```

If you do not set `GITHUB_COPILOT_CLIENT_SECRET`, GitHub Copilot sign-in still works, but expired GitHub OAuth sessions must be refreshed by logging in again.

### Check Status

```bash
bb login    # Shows ✓/✗ for each provider
```

## Selecting a Model

### CLI Flags

```bash
bb --model sonnet                                # Fuzzy match
bb --model claude-sonnet-4-6                     # Exact model ID
bb --model anthropic/claude-sonnet-4-6           # Provider/model
bb --model sonnet:high                           # Model with thinking level
bb --provider google --model gemini-3.1-pro-preview # Explicit provider
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
/model anthropic/claude-sonnet-4-6:low
```

`/model` now accepts common provider/model and thinking-suffix formats directly during a conversation.

### Default Model

In `settings.json`:
```json
{
  "default_provider": "anthropic",
  "default_model": "claude-opus-4-6",
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
