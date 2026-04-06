use super::*;
use crate::agent::{AgentModel, RuntimeAgentEvent};
use std::sync::{Arc, Mutex as StdMutex};

#[tokio::test]
async fn unsubscribe_removes_only_target_listener() {
    let agent = Agent::new(AgentOptions::default());
    {
        let mut inner = agent.inner.lock().await;
        inner.active_run = Some(ActiveRun::new());
    }

    let events_a = Arc::new(StdMutex::new(Vec::new()));
    let events_b = Arc::new(StdMutex::new(Vec::new()));

    let unsubscribe_a = agent
        .subscribe({
            let events = events_a.clone();
            move |event, _signal| {
                let events = events.clone();
                Box::pin(async move {
                    events.lock().unwrap().push(event);
                })
            }
        })
        .await;
    let _unsubscribe_b = agent
        .subscribe({
            let events = events_b.clone();
            move |event, _signal| {
                let events = events.clone();
                Box::pin(async move {
                    events.lock().unwrap().push(event);
                })
            }
        })
        .await;

    unsubscribe_a();
    tokio::task::yield_now().await;

    agent
        .process_event(RuntimeAgentEvent::MessageStart {
            message: AgentMessage::assistant_error(
                &AgentModel::default(),
                "stop",
                "boom".to_string(),
            ),
        })
        .await
        .unwrap();

    assert!(events_a.lock().unwrap().is_empty());
    assert_eq!(events_b.lock().unwrap().len(), 1);
}
