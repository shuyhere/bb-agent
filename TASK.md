# A3: Fix Anthropic thinking/reasoning + OpenAI quirks + retry + model registry

Working dir: `/tmp/bb-final/a3-anthropic-thinking/`
BB-Agent Rust project. Read REVIEW.md for what's missing.

## Tasks

### 1. Fix Anthropic thinking in `crates/provider/src/anthropic.rs`

Currently the `thinking` field in CompletionRequest exists but isn't applied to the Anthropic API body.

Add thinking support:
```rust
// In the body building:
if let Some(ref thinking) = request.thinking {
    let budget = match thinking.as_str() {
        "minimal" => 1024,
        "low" => 2048,
        "medium" => 8192,
        "high" => 16384,
        "xhigh" => 32768,
        _ => 8192,
    };
    body["thinking"] = json!({
        "type": "enabled",
        "budget_tokens": budget,
    });
    // When thinking is enabled, Anthropic requires max_tokens to be higher
    if request.max_tokens.unwrap_or(0) < (budget as u32 + 4096) {
        body["max_tokens"] = json!(budget + 4096);
    }
}
```

### 2. Fix OpenAI provider quirks in `crates/provider/src/openai.rs`

Add provider-specific handling:
```rust
// In body building:
// Some providers don't support max_completion_tokens, use max_tokens instead
let is_groq = options.base_url.contains("groq.com");
let is_ollama = options.base_url.contains("localhost") || options.base_url.contains("127.0.0.1");

if let Some(max_tokens) = request.max_tokens {
    if is_groq || is_ollama {
        body["max_tokens"] = json!(max_tokens);
    } else {
        body["max_completion_tokens"] = json!(max_tokens);
    }
}

// Add reasoning_effort for OpenAI models that support it
if let Some(ref thinking) = request.thinking {
    let effort = match thinking.as_str() {
        "low" | "minimal" => "low",
        "medium" => "medium",
        "high" | "xhigh" => "high",
        _ => "medium",
    };
    body["reasoning_effort"] = json!(effort);
}
```

### 3. Add retry with exponential backoff in both providers

Create `crates/provider/src/retry.rs`:
```rust
use std::time::Duration;
use tokio::time::sleep;
use bb_core::error::{BbError, BbResult};

pub async fn with_retry<F, Fut, T>(max_retries: u32, f: F) -> BbResult<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = BbResult<T>>,
{
    let mut last_err = BbError::Provider("No attempts made".into());
    for attempt in 0..max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_err = e;
                if attempt < max_retries - 1 {
                    let delay = Duration::from_millis(1000 * 2u64.pow(attempt));
                    tracing::warn!("Provider request failed (attempt {}), retrying in {:?}", attempt + 1, delay);
                    sleep(delay).await;
                }
            }
        }
    }
    Err(last_err)
}
```

Wrap the HTTP request in both `anthropic.rs` and `openai.rs` with `with_retry(3, || async { ... })`.

### 4. Expand model registry in `crates/provider/src/registry.rs`

Add more models to the hardcoded list. At minimum add:
- `claude-3-5-haiku-20241022` (Anthropic, cheap fast model)
- `claude-3-7-sonnet-20250219` (Anthropic, older sonnet)
- `gpt-4-turbo` (OpenAI)
- `o1-mini` (OpenAI reasoning)
- `llama-3.1-8b-instant` (Groq, fast)
- `mixtral-8x7b-32768` (Groq)

### 5. Update `crates/provider/src/lib.rs`

Add `pub mod retry;`

### 6. Tests

```rust
#[tokio::test]
async fn test_retry_succeeds_on_second_attempt() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();
    let result = with_retry(3, || {
        let c = c.clone();
        async move {
            let attempt = c.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                Err(BbError::Provider("temporary".into()))
            } else {
                Ok(42)
            }
        }
    }).await;
    assert_eq!(result.unwrap(), 42);
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}
```

### Build and test
```bash
cd /tmp/bb-final/a3-anthropic-thinking
cargo build && cargo test
git add -A && git commit -m "A3: anthropic thinking + openai quirks + retry + models"
```
