use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use super::abort::AgentAbortSignal;
use super::data::{
    AfterToolCallContext, AfterToolCallResult, AgentContextSnapshot, AgentLoopConfig, AgentMessage,
    BeforeToolCallContext, BeforeToolCallResult,
};
use super::events::{AgentEventSink, RuntimeAgentEvent};

pub type AgentFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
pub type Listener =
    Arc<dyn Fn(RuntimeAgentEvent, AgentAbortSignal) -> AgentFuture<()> + Send + Sync>;
pub type ConvertToLlmFn =
    Arc<dyn Fn(Vec<AgentMessage>) -> AgentFuture<Vec<AgentMessage>> + Send + Sync>;
pub type TransformContextFn = Arc<
    dyn Fn(Vec<AgentMessage>, AgentAbortSignal) -> AgentFuture<Vec<AgentMessage>> + Send + Sync,
>;
pub type BeforeToolCallFn = Arc<
    dyn Fn(BeforeToolCallContext, AgentAbortSignal) -> AgentFuture<Option<BeforeToolCallResult>>
        + Send
        + Sync,
>;
pub type AfterToolCallFn = Arc<
    dyn Fn(AfterToolCallContext, AgentAbortSignal) -> AgentFuture<Option<AfterToolCallResult>>
        + Send
        + Sync,
>;
pub type StreamFn = Arc<
    dyn Fn(
            AgentContextSnapshot,
            AgentLoopConfig,
            AgentEventSink,
            AgentAbortSignal,
        ) -> AgentFuture<anyhow::Result<()>>
        + Send
        + Sync,
>;
