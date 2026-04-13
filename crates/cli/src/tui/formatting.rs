use bb_core::types::{AssistantContent, ContentBlock};

pub(super) fn format_user_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::Image { mime_type, .. } => format!("[{mime_type} attachment]"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn text_from_blocks(blocks: &[ContentBlock], separator: &str) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(separator)
}

pub(super) fn format_assistant_text(message: &bb_core::types::AssistantMessage) -> String {
    message
        .content
        .iter()
        .filter_map(|block| match block {
            AssistantContent::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
