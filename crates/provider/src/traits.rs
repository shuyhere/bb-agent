use async_trait::async_trait;
use bb_core::error::BbResult;
use tokio::sync::mpsc;

use crate::types::{CompletionRequest, RequestOptions, StreamEvent};

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
