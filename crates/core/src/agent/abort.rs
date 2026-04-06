use std::sync::Arc;

use tokio::sync::Notify;

#[derive(Clone)]
pub struct AgentAbortSignal {
    state: Arc<AbortState>,
}

struct AbortState {
    aborted: std::sync::atomic::AtomicBool,
    notify: Notify,
}

impl AgentAbortSignal {
    pub fn aborted(&self) -> bool {
        self.state.aborted.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        if self.aborted() {
            return;
        }
        self.state.notify.notified().await;
    }
}

#[derive(Clone)]
pub struct AgentAbortController {
    state: Arc<AbortState>,
}

impl Default for AgentAbortController {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAbortController {
    pub fn new() -> Self {
        Self {
            state: Arc::new(AbortState {
                aborted: std::sync::atomic::AtomicBool::new(false),
                notify: Notify::new(),
            }),
        }
    }

    pub fn signal(&self) -> AgentAbortSignal {
        AgentAbortSignal {
            state: Arc::clone(&self.state),
        }
    }

    pub fn abort(&self) {
        self.state
            .aborted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.state.notify.notify_waiters();
    }
}
