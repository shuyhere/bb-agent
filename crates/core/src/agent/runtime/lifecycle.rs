use std::sync::Arc;

use super::super::abort::AgentAbortSignal;
use super::super::callbacks::AgentFuture;
use super::super::data::{AgentContextSnapshot, AgentLoopConfig, AgentMessage, ThinkingLevel};
use super::super::events::RuntimeAgentEvent;
use super::super::helpers::context_with_prompt;
use super::{Agent, LoopConfigOptions};

impl Agent {
    pub(super) async fn run_prompt_messages(
        &self,
        messages: Vec<AgentMessage>,
        skip_initial_steering_poll: bool,
    ) -> anyhow::Result<()> {
        let messages_for_run = messages;
        self.run_with_lifecycle(move |agent, signal| {
            let messages = messages_for_run.clone();
            Box::pin(async move {
                let context = agent.create_context_snapshot().await;
                let config = agent
                    .create_loop_config(LoopConfigOptions {
                        skip_initial_steering_poll,
                    })
                    .await;
                let sink = agent.event_sink();
                let stream_fn = { agent.inner.lock().await.stream_fn.clone() };
                (stream_fn)(context_with_prompt(context, messages), config, sink, signal).await
            })
        })
        .await
    }

    pub(super) async fn run_continuation(&self) -> anyhow::Result<()> {
        self.run_with_lifecycle(|agent, signal| {
            Box::pin(async move {
                let context = agent.create_context_snapshot().await;
                let config = agent.create_loop_config(LoopConfigOptions::default()).await;
                let sink = agent.event_sink();
                let stream_fn = { agent.inner.lock().await.stream_fn.clone() };
                (stream_fn)(context, config, sink, signal).await
            })
        })
        .await
    }

    async fn create_context_snapshot(&self) -> AgentContextSnapshot {
        let inner = self.inner.lock().await;
        AgentContextSnapshot {
            system_prompt: inner.state.system_prompt.clone(),
            messages: inner.state.messages.clone(),
            tools: inner.state.tools.clone(),
        }
    }

    async fn create_loop_config(&self, options: LoopConfigOptions) -> AgentLoopConfig {
        let inner = self.inner.lock().await;
        let skip_initial_steering_poll = Arc::new(std::sync::atomic::AtomicBool::new(
            options.skip_initial_steering_poll,
        ));
        let steering_agent = self.clone();
        let follow_up_agent = self.clone();
        AgentLoopConfig {
            model: inner.state.model.clone(),
            reasoning: match inner.state.thinking_level {
                ThinkingLevel::Off => None,
                level => Some(level),
            },
            session_id: inner.session_id.clone(),
            transport: inner.transport,
            thinking_budgets: inner.thinking_budgets.clone(),
            max_retry_delay_ms: inner.max_retry_delay_ms,
            tool_execution: inner.tool_execution,
            before_tool_call: inner.before_tool_call.clone(),
            after_tool_call: inner.after_tool_call.clone(),
            convert_to_llm: Some(inner.convert_to_llm.clone()),
            transform_context: inner.transform_context.clone(),
            get_api_key: inner.get_api_key.clone(),
            get_steering_messages: Some(Arc::new(move || {
                let agent = steering_agent.clone();
                let skip_flag = skip_initial_steering_poll.clone();
                Box::pin(async move {
                    let should_skip = skip_flag.swap(false, std::sync::atomic::Ordering::SeqCst);
                    if should_skip {
                        Vec::new()
                    } else {
                        let mut inner = agent.inner.lock().await;
                        inner.steering_queue.drain()
                    }
                })
            })),
            get_follow_up_messages: Some(Arc::new(move || {
                let agent = follow_up_agent.clone();
                Box::pin(async move {
                    let mut inner = agent.inner.lock().await;
                    inner.follow_up_queue.drain()
                })
            })),
        }
    }

    async fn run_with_lifecycle<F>(&self, executor: F) -> anyhow::Result<()>
    where
        F: FnOnce(Agent, AgentAbortSignal) -> AgentFuture<anyhow::Result<()>> + Send + 'static,
    {
        let active_run = {
            let mut inner = self.inner.lock().await;
            if inner.active_run.is_some() {
                anyhow::bail!("Agent is already processing.");
            }
            let active_run = super::ActiveRun::new();
            inner.active_run = Some(active_run.clone());
            inner.state.is_streaming = true;
            inner.state.streaming_message = None;
            inner.state.error_message = None;
            active_run
        };

        let result = executor(self.clone(), active_run.signal.clone()).await;
        if let Err(error) = result {
            self.handle_run_failure(error, active_run.signal.aborted())
                .await?;
        }
        self.finish_run().await;
        Ok(())
    }

    async fn handle_run_failure(&self, error: anyhow::Error, aborted: bool) -> anyhow::Result<()> {
        let failure_message = {
            let inner = self.inner.lock().await;
            AgentMessage::assistant_error(
                &inner.state.model,
                if aborted { "aborted" } else { "error" },
                error.to_string(),
            )
        };

        {
            let mut inner = self.inner.lock().await;
            inner.state.messages.push(failure_message.clone());
            inner.state.error_message = failure_message.error_message.clone();
        }

        self.process_event(RuntimeAgentEvent::AgentEnd {
            messages: vec![failure_message],
        })
        .await
    }

    async fn finish_run(&self) {
        let active_run = {
            let mut inner = self.inner.lock().await;
            inner.state.is_streaming = false;
            inner.state.streaming_message = None;
            inner.state.pending_tool_calls.clear();
            inner.active_run.take()
        };

        if let Some(active_run) = active_run {
            active_run.finish();
        }
    }
}
