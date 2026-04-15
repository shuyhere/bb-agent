use super::transcript_validation::validate_and_repair_messages_for_provider;

pub fn messages_to_provider(messages: &[crate::types::AgentMessage]) -> Vec<serde_json::Value> {
    validate_and_repair_messages_for_provider(messages).messages
}
