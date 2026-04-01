use super::data::AgentMessage;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueMode {
    All,
    OneAtATime,
}

impl Default for QueueMode {
    fn default() -> Self {
        Self::OneAtATime
    }
}

#[derive(Clone, Debug)]
pub struct PendingMessageQueue {
    mode: QueueMode,
    messages: Vec<AgentMessage>,
}

impl PendingMessageQueue {
    pub fn new(mode: QueueMode) -> Self {
        Self {
            mode,
            messages: Vec::new(),
        }
    }

    pub fn mode(&self) -> QueueMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: QueueMode) {
        self.mode = mode;
    }

    pub fn enqueue(&mut self, message: AgentMessage) {
        self.messages.push(message);
    }

    pub fn has_items(&self) -> bool {
        !self.messages.is_empty()
    }

    pub fn drain(&mut self) -> Vec<AgentMessage> {
        match self.mode {
            QueueMode::All => std::mem::take(&mut self.messages),
            QueueMode::OneAtATime => {
                if self.messages.is_empty() {
                    Vec::new()
                } else {
                    vec![self.messages.remove(0)]
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }
}
