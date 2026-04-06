mod compaction;
mod retry_bash;
mod tree;

use serde::{Deserialize, Serialize};

use super::session_tree::SessionTreeState;
use super::types::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentSessionRuntime {
    pub model: Option<RuntimeModelRef>,
    pub messages: Vec<RuntimeMessage>,
    pub session_tree: SessionTreeState,
    pub compaction_state: CompactionState,
    pub retry_state: RetryState,
    pub bash_state: BashExecutionState,
    pub tree_state: TreeNavigationState,
    pub queued_continue_requested: bool,
    pub emitted_events: Vec<RuntimeEvent>,
}

impl AgentSessionRuntime {
    fn emit(&mut self, event: RuntimeEvent) {
        self.emitted_events.push(event);
    }

    fn remove_last_assistant_message(&mut self) {
        if matches!(self.messages.last(), Some(RuntimeMessage::Assistant(_))) {
            self.messages.pop();
        }
    }

    fn resolve_retry(&mut self) {
        self.retry_state.in_progress = false;
    }
}
