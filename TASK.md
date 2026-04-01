# A4: Implement Google Generative AI provider

Working dir: `/tmp/bb-final/a4-google-provider/`
BB-Agent Rust project.

## Task: Create `crates/provider/src/google.rs`

Implement the Google Generative AI API provider (Gemini models).

### API endpoint
```
POST https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?key={apiKey}&alt=sse
```

### Request format
```json
{
    "contents": [
        { "role": "user", "parts": [{ "text": "Hello" }] },
        { "role": "model", "parts": [{ "text": "Hi!" }] }
    ],
    "systemInstruction": {
        "parts": [{ "text": "You are a helpful assistant" }]
    },
    "tools": [{
        "functionDeclarations": [{
            "name": "read",
            "description": "Read a file",
            "parameters": { "type": "OBJECT", "properties": { "path": { "type": "STRING" } }, "required": ["path"] }
        }]
    }],
    "generationConfig": {
        "maxOutputTokens": 16384
    }
}
```

### Message role mapping
- user → `user`
- assistant → `model`
- tool results → `user` with `functionResponse` parts

### Tool call format
Google returns tool calls as `functionCall` parts in model responses:
```json
{ "functionCall": { "name": "read", "args": { "path": "foo.rs" } } }
```

### Tool result format
```json
{
    "role": "user",
    "parts": [{
        "functionResponse": {
            "name": "read",
            "response": { "content": "file contents here" }
        }
    }]
}
```

### SSE streaming
Google streams via SSE with `data: {...}` lines. Each chunk has:
```json
{
    "candidates": [{
        "content": {
            "parts": [{ "text": "delta text" }],
            "role": "model"
        }
    }],
    "usageMetadata": {
        "promptTokenCount": 100,
        "candidatesTokenCount": 50
    }
}
```

### Implementation

```rust
pub struct GoogleProvider {
    client: Client,
}

#[async_trait]
impl Provider for GoogleProvider {
    fn name(&self) -> &str { "google" }

    async fn stream(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?key={}&alt=sse",
            options.base_url.trim_end_matches('/'),
            request.model,
            options.api_key,
        );

        // Convert messages to Google format
        let contents = convert_messages_google(&request.messages);

        // Convert tools to Google format
        let tools = convert_tools_google(&request.tools);

        let mut body = json!({
            "contents": contents,
            "generationConfig": {
                "maxOutputTokens": request.max_tokens.unwrap_or(16384),
            }
        });

        if !request.system_prompt.is_empty() {
            body["systemInstruction"] = json!({
                "parts": [{ "text": request.system_prompt }]
            });
        }

        if !tools.is_empty() {
            body["tools"] = json!([{ "functionDeclarations": tools }]);
        }

        // Send request and parse SSE stream...
    }
}
```

### Update `crates/provider/src/lib.rs`
Add `pub mod google;`

### Update `crates/cli/src/run.rs` (or interactive.rs)
Add Google to provider selection:
```rust
ApiType::GoogleGenerative => Box::new(GoogleProvider::new()),
```

### Tests
- Test message format conversion
- Test tool format conversion
- Test SSE parsing

### Build and test
```bash
cd /tmp/bb-final/a4-google-provider
cargo build && cargo test
git add -A && git commit -m "A4: implement Google Generative AI provider"
```
