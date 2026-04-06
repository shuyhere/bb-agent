use super::super::data::AgentMessageRole;
use super::super::events::{AgentEventSink, RuntimeAgentEvent};
use super::Agent;

impl Agent {
    pub(super) async fn process_event(&self, event: RuntimeAgentEvent) -> anyhow::Result<()> {
        let (listeners, signal) = {
            let mut inner = self.inner.lock().await;
            match &event {
                RuntimeAgentEvent::MessageStart { message } => {
                    inner.state.streaming_message = Some(message.clone());
                }
                RuntimeAgentEvent::MessageUpdate { message } => {
                    inner.state.streaming_message = Some(message.clone());
                }
                RuntimeAgentEvent::MessageEnd { message } => {
                    inner.state.streaming_message = None;
                    inner.state.messages.push(message.clone());
                }
                RuntimeAgentEvent::ToolExecutionStart { tool_call_id } => {
                    inner.state.pending_tool_calls.insert(tool_call_id.clone());
                }
                RuntimeAgentEvent::ToolExecutionEnd { tool_call_id } => {
                    inner.state.pending_tool_calls.remove(tool_call_id);
                }
                RuntimeAgentEvent::TurnEnd { message } => {
                    if matches!(message.role, AgentMessageRole::Assistant)
                        && let Some(error_message) = &message.error_message
                    {
                        inner.state.error_message = Some(error_message.clone());
                    }
                }
                RuntimeAgentEvent::AgentEnd { .. } => {
                    inner.state.streaming_message = None;
                }
            }

            let Some(active_run) = inner.active_run.clone() else {
                anyhow::bail!("Agent listener invoked outside active run");
            };
            (
                inner
                    .listeners
                    .iter()
                    .map(|(_, listener)| listener.clone())
                    .collect::<Vec<_>>(),
                active_run.signal,
            )
        };

        for listener in listeners {
            listener(event.clone(), signal.clone()).await;
        }
        Ok(())
    }

    pub(super) fn event_sink(&self) -> AgentEventSink {
        let agent = self.clone();
        AgentEventSink::new(move |event| {
            let agent = agent.clone();
            Box::pin(async move { agent.process_event(event).await })
        })
    }
}
