use bb_tui::theme::theme;

use super::*;

impl InteractiveMode {
    pub(super) fn sync_pending_render_state(&mut self) {
        let queued = self
            .queues.compaction_queued_messages
            .iter()
            .map(|message| RenderQueuedMessage {
                text: message.text.clone(),
                mode: match message.kind {
                    QueuedMessageKind::Steer => QueuedMessageMode::Steer,
                    QueuedMessageKind::FollowUp => QueuedMessageMode::FollowUp,
                },
            })
            .collect::<Vec<_>>();
        let steering: Vec<String> = self.queues.steering_queue.iter().cloned().collect();
        let follow_up: Vec<String> = self.queues.follow_up_queue.iter().cloned().collect();
        let pending = InteractiveRenderState::collect_pending_messages(&steering, &follow_up, &queued);
        self.controller.session.pending_messages = pending.clone();
        self.render_state_mut()
            .update_pending_messages_display(&pending);
    }

    pub(super) fn render_items_to_lines(items: &mut [ChatItem], width: u16) -> Vec<String> {
        let t = theme();
        let dim = &t.dim;
        let reset = &t.reset;
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
            .iter_mut()
            .flat_map(|item| match item {
                ChatItem::Spacer => vec![String::new()],
                ChatItem::UserMessage(text) => {
                    let user_bg = &t.user_msg_bg;
                    vec![String::new(), format!("{user_bg} {text}\x1b[K{reset}"), String::new()]
                }
                ChatItem::AssistantMessage(component) => component
                    .render_lines(content_width as u16)  // uses internal cache
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
                ChatItem::SystemMessage(text) => {
                    let yellow = &t.yellow;
                    word_wrap(&format!("{yellow} {text}{reset}"), width.max(1) as usize)
                }
                ChatItem::ErrorMessage(text) => {
                    word_wrap(&format!("{} Error: {text}{}", t.error, reset), width.max(1) as usize)
                }
            })
            .collect()
    }

    pub(super) fn chat_render_lines(&mut self) -> Vec<String> {
        let width = self.ui.tui.columns();
        let items = &mut self.controller.session.render_state.chat_items;
        let cached_count = self.render_cache.cached_chat_line_count;
        let cached_width = self.render_cache.cached_chat_width;
        let cache_valid = cached_count > 0
            && cached_count <= items.len()
            && cached_width == width
            && !self.render_cache.cached_chat_lines_prefix.is_empty();

        let total_capacity = if cache_valid {
            self.render_cache.cached_chat_lines_prefix.len() + 50
        } else {
            items.len() * 5
        };
        let mut lines = Vec::with_capacity(total_capacity);

        if cache_valid {
            // Copy cached prefix and only re-render new/streaming items.
            lines.extend_from_slice(&self.render_cache.cached_chat_lines_prefix);
            let tail = &mut items[cached_count..];
            if !tail.is_empty() {
                lines.extend(Self::render_items_to_lines(tail, width));
            }
        } else {
            lines.extend(Self::render_items_to_lines(items, width));
        }

        for line in &self.render_cache.chat_lines {
            lines.extend(word_wrap(line, width.max(1) as usize));
        }
        lines
    }

    pub(super) fn pending_render_lines(&mut self) -> Vec<String> {
        let width = self.ui.tui.columns();
        let mut lines = Self::render_items_to_lines(&mut self.controller.session.render_state.pending_items, width);
        for line in &self.render_cache.pending_lines {
            lines.extend(word_wrap(line, width.max(1) as usize));
        }
        lines
    }
}
