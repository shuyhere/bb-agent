use super::*;

impl InteractiveMode {
    pub(super) fn sync_pending_render_state(&mut self) {
        let queued = self
            .compaction_queued_messages
            .iter()
            .map(|message| RenderQueuedMessage {
                text: message.text.clone(),
                mode: match message.kind {
                    QueuedMessageKind::Steer => QueuedMessageMode::Steer,
                    QueuedMessageKind::FollowUp => QueuedMessageMode::FollowUp,
                },
            })
            .collect::<Vec<_>>();
        let steering: Vec<String> = self.steering_queue.iter().cloned().collect();
        let follow_up: Vec<String> = self.follow_up_queue.iter().cloned().collect();
        let pending = InteractiveRenderState::collect_pending_messages(&steering, &follow_up, &queued);
        self.controller.session.pending_messages = pending.clone();
        self.render_state_mut()
            .update_pending_messages_display(&pending);
    }

    pub(super) fn render_items_to_lines(items: &[ChatItem], width: u16) -> Vec<String> {
        let dim = "\x1b[90m";
        let reset = "\x1b[0m";
        let content_width = width.saturating_sub(1).max(1) as usize;
        let wrap_prefixed = |line: &str| -> Vec<String> {
            if line.is_empty() {
                vec![String::new()]
            } else {
                word_wrap(line, content_width)
                    .into_iter()
                    .map(|l| format!(" {l}"))
                    .collect()
            }
        };

        items
            .iter()
            .flat_map(|item| match item {
                ChatItem::Spacer => vec![String::new()],
                ChatItem::UserMessage(text) => {
                    let user_bg = "\x1b[48;2;52;53;65m";
                    vec![String::new(), format!("{user_bg} {text}\x1b[K{reset}"), String::new()]
                }
                ChatItem::AssistantMessage(component) => component
                    .render_lines(content_width as u16)
                    .into_iter()
                    .map(|line| if line.is_empty() { String::new() } else { format!(" {line}") })
                    .collect(),
                ChatItem::ToolExecution(component) => component
                    .render_lines(width)
                    .into_iter()
                    .collect(),
                ChatItem::BashExecution(component) => component
                    .render_lines()
                    .iter()
                    .flat_map(|l| wrap_prefixed(l))
                    .collect(),
                ChatItem::CustomMessage { text, .. } => word_wrap(&format!("{dim} {text}{reset}"), width.max(1) as usize),
                ChatItem::CompactionSummary(summary) => word_wrap(&format!("{dim} [c] {summary}{reset}"), width.max(1) as usize),
                ChatItem::BranchSummary(summary) => word_wrap(&format!("{dim} [b] {summary}{reset}"), width.max(1) as usize),
                ChatItem::PendingMessageLine(line) => wrap_prefixed(line),
            })
            .collect()
    }

    pub(super) fn chat_render_lines(&self) -> Vec<String> {
        let width = self.ui.columns();
        let mut lines = Self::render_items_to_lines(&self.render_state().chat_items, width);
        for line in &self.chat_lines {
            lines.extend(word_wrap(line, width.max(1) as usize));
        }
        lines
    }

    pub(super) fn pending_render_lines(&self) -> Vec<String> {
        let width = self.ui.columns();
        let mut lines = Self::render_items_to_lines(&self.render_state().pending_items, width);
        for line in &self.pending_lines {
            lines.extend(word_wrap(line, width.max(1) as usize));
        }
        lines
    }
}
