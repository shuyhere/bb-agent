# Built-in Tools

BB-Agent comes with 10 built-in tools that the AI can use during conversations.

## File Tools

### `read`
Read file contents (text and images). Supports offset/limit for large files.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | ✅ | File path (relative or absolute) |
| `offset` | number | | Line number to start from (1-indexed) |
| `limit` | number | | Max lines to read (default: 2000) |

- Text files: truncated at 2000 lines or 50KB
- Images (jpg, png, gif, webp): sent as base64 attachments

### `write`
Write content to a file. Creates parent directories automatically.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | ✅ | File path |
| `content` | string | ✅ | Content to write |

### `edit`
Make precise text replacements in a file. Each `oldText` must match exactly one location.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | ✅ | File path |
| `edits` | array | ✅ | Array of `{oldText, newText}` replacements |

- Edits must not overlap
- Returns a unified diff of changes
- If `oldText` matches 0 or 2+ locations, the edit is rejected

## Shell Tools

### `bash`
Execute a shell command via `bash -c`.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `command` | string | ✅ | Command to execute |
| `timeout` | number | | Timeout in seconds |

- Output truncated at 2000 lines or 50KB
- Supports cancellation
- Runs in the project's working directory

### `find`
Search for files by name pattern.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `pattern` | string | ✅ | Glob or name pattern |
| `path` | string | | Directory to search (default: cwd) |

### `grep`
Search file contents with regex.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `pattern` | string | ✅ | Regex pattern |
| `path` | string | | Directory or file to search (default: cwd) |
| `include` | string | | File glob filter (e.g., `*.rs`) |

### `ls`
List directory contents.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `path` | string | | Directory to list (default: cwd) |

## Web Tools

### `web_search`
Search the public web via DuckDuckGo. Returns titles, URLs, and snippets.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `query` | string | ✅ | Search query |
| `max_results` | number | | Max results (default: 10) |

### `web_fetch`
Fetch a web page and extract main content as text.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `url` | string | ✅ | URL to fetch |
| `max_chars` | number | | Max characters (default: 20000) |
| `timeout` | number | | Timeout in seconds |

- Extracts `<main>`, `<article>`, or `<body>` content
- Strips HTML tags, scripts, styles
- Returns citation markdown for source attribution

### `browser_fetch`
Fetch a page using a real headless Chrome/Chromium browser. Use when `web_fetch` is blocked by JavaScript, anti-bot pages, or dynamic content.

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `url` | string | ✅ | URL to fetch |
| `max_chars` | number | | Max characters (default: 20000) |
| `timeout` | number | | Timeout in seconds (default: 25) |

Requires Chrome or Chromium installed. Set `BB_BROWSER` env var to specify a custom binary path.

## Restricting Tools

### Via CLI
```bash
bb --tools read,bash,edit,write    # Only these tools
bb --no-tools                       # Disable all tools
```

### Via settings.json
```json
{
  "tools": ["read", "bash", "edit", "write"]
}
```

Set `"tools": null` to enable all tools (default).
