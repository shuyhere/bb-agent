use bb_core::types::CacheMetricsSource;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// A completion request to send to a provider.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub system_prompt: String,
    pub messages: Vec<serde_json::Value>,
    pub tools: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_tool_schemas: Vec<serde_json::Value>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    /// Thinking level: "low", "medium", "high", or None for disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ProviderRetryEvent {
    Start {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        error_message: String,
    },
    End {
        success: bool,
        attempt: u32,
        final_error: Option<String>,
    },
}

pub type RetryCallback = Arc<dyn Fn(ProviderRetryEvent) + Send + Sync>;

/// Options for a provider request.
pub struct RequestOptions {
    pub api_key: String,
    pub base_url: String,
    pub headers: std::collections::HashMap<String, String>,
    pub cancel: CancellationToken,
    pub retry_callback: Option<RetryCallback>,
    pub max_retries: u32,
    pub retry_base_delay_ms: u64,
    pub max_retry_delay_ms: u64,
}

/// A streaming event from the provider.
#[derive(Clone, Debug)]
pub enum StreamEvent {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        text: String,
    },
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        arguments_delta: String,
    },
    ToolCallEnd {
        id: String,
    },
    ServerToolUseStart {
        id: String,
        name: String,
    },
    ServerToolUseDelta {
        id: String,
        arguments_delta: String,
    },
    ServerToolUseEnd {
        id: String,
    },
    ServerToolResult {
        tool_use_id: String,
        name: String,
        result: serde_json::Value,
        is_error: bool,
    },
    Usage(UsageInfo),
    Done,
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_metrics_source: CacheMetricsSource,
}
