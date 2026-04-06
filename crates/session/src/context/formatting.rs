use bb_core::types::{
    AgentMessage, BranchSummaryMessage, CustomMessage, ModelInfo, SessionEntry, ThinkingLevel,
};

pub(super) fn append_message(messages: &mut Vec<AgentMessage>, entry: &SessionEntry) {
    match entry {
        SessionEntry::Message { message, .. } => {
            messages.push(message.clone());
        }
        SessionEntry::BranchSummary {
            summary,
            from_id,
            base,
            ..
        } => {
            messages.push(AgentMessage::BranchSummary(BranchSummaryMessage {
                summary: summary.clone(),
                from_id: from_id.as_str().to_string(),
                timestamp: base.timestamp.timestamp_millis(),
            }));
        }
        SessionEntry::CustomMessage {
            custom_type,
            content,
            display,
            details,
            base,
            ..
        } => {
            messages.push(AgentMessage::Custom(CustomMessage {
                custom_type: custom_type.clone(),
                content: content.clone(),
                display: *display,
                details: details.clone(),
                timestamp: base.timestamp.timestamp_millis(),
            }));
        }
        _ => {}
    }
}

pub(super) fn update_settings(
    entry: &SessionEntry,
    model: &mut Option<ModelInfo>,
    thinking_level: &mut ThinkingLevel,
) {
    match entry {
        SessionEntry::ModelChange {
            provider, model_id, ..
        } => {
            *model = Some(ModelInfo {
                provider: provider.clone(),
                model_id: model_id.clone(),
            });
        }
        SessionEntry::ThinkingLevelChange {
            thinking_level: level,
            ..
        } => {
            *thinking_level = *level;
        }
        SessionEntry::Message {
            message: AgentMessage::Assistant(asst),
            ..
        } => {
            *model = Some(ModelInfo {
                provider: asst.provider.clone(),
                model_id: asst.model.clone(),
            });
        }
        _ => {}
    }
}
