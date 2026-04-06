# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.0.x   | ✅        |

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly:

1. **Do NOT open a public GitHub issue**
2. Email: [open an issue with the "security" label as private](https://github.com/shuyhere/bb-agent/security/advisories/new)

Please include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

## Security Considerations

### API Keys & Credentials

- API keys are stored in `~/.bb-agent/auth.json` with file permissions restricted to owner-only (`chmod 600`)
- Keys are never logged or included in tracing output
- OAuth tokens are auto-refreshed when expired
- Environment variables (`ANTHROPIC_API_KEY`, etc.) are supported as an alternative to the auth store

### Tool Execution

BB-Agent executes commands on your behalf using the `bash` tool. The agent can:
- Read and write files anywhere accessible to your user
- Execute arbitrary shell commands
- Make network requests

This is by design — BB-Agent is a coding assistant that operates with your permissions, similar to Claude Code, Cursor, and other AI coding tools.

### Extensions

JS/TS extensions run as child processes and can:
- Register custom tools
- Intercept input/output via hooks
- Access the session transcript

Only install extensions from trusted sources.

### Network

- LLM API calls go to the configured provider's endpoint
- `web_search` uses DuckDuckGo
- `web_fetch` makes HTTP requests to user-specified URLs
- `browser_fetch` launches a headless Chrome/Chromium instance
- The OAuth callback server binds only to `127.0.0.1` (localhost)
