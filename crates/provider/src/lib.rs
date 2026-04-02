pub mod error;
pub mod registry;
pub mod resolver;
pub mod openai;
pub mod anthropic;
pub mod google;
pub mod streaming;
pub mod retry;
pub mod transforms;

use async_trait::async_trait;
use bb_core::error::BbResult;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// A completion request to send to a provider.
#[derive(Clone, Debug, Serialize)]
pub struct CompletionRequest {
    pub system_prompt: String,
    pub messages: Vec<serde_json::Value>,
    pub tools: Vec<serde_json::Value>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    /// Thinking level: "low", "medium", "high", or None for disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

/// Options for a provider request.
pub struct RequestOptions {
    pub api_key: String,
    pub base_url: String,
    pub headers: std::collections::HashMap<String, String>,
    pub cancel: CancellationToken,
}

/// A streaming event from the provider.
#[derive(Clone, Debug)]
pub enum StreamEvent {
    TextDelta { text: String },
    ThinkingDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, arguments_delta: String },
    ToolCallEnd { id: String },
    Usage(UsageInfo),
    Done,
    Error { message: String },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

/// Provider trait — implemented by each API backend.
/// Returns events via channel for real-time streaming.
#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    /// Non-streaming: returns all events at once.
    async fn complete(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>>;

    /// Streaming: sends events to channel as they arrive.
    async fn stream(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()>;
}
