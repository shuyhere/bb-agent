/// Serialize an Event into a JSON value suitable for the plugin host.
pub(super) fn serialize_event(event: &bb_hooks::Event) -> serde_json::Value {
    use bb_hooks::Event;

    let event_type = event.event_type();
    match event {
        Event::SessionStart => serde_json::json!({ "type": event_type }),
        Event::SessionShutdown => serde_json::json!({ "type": event_type }),
        Event::AgentEnd => serde_json::json!({ "type": event_type }),
        Event::TurnStart { turn_index } => {
            serde_json::json!({ "type": event_type, "turn_index": turn_index })
        }
        Event::TurnEnd { turn_index } => {
            serde_json::json!({ "type": event_type, "turn_index": turn_index })
        }
        Event::ToolCall(tc) => serde_json::json!({
            "type": event_type,
            "tool_call_id": tc.tool_call_id,
            "tool_name": tc.tool_name,
            "input": tc.input,
        }),
        Event::ToolResult(tr) => serde_json::json!({
            "type": event_type,
            "tool_call_id": tr.tool_call_id,
            "tool_name": tr.tool_name,
            "input": tr.input,
            "content": tr.content,
            "details": tr.details,
            "is_error": tr.is_error,
        }),
        Event::BeforeAgentStart {
            prompt,
            system_prompt,
        } => serde_json::json!({
            "type": event_type,
            "prompt": prompt,
            "system_prompt": system_prompt,
        }),
        Event::SessionBeforeCompact(prep) => serde_json::json!({
            "type": event_type,
            "preparation": {
                "firstKeptEntryId": prep.first_kept_entry_id,
                "tokensBefore": prep.tokens_before,
            },
        }),
        Event::SessionCompact { from_plugin } => serde_json::json!({
            "type": event_type,
            "from_plugin": from_plugin,
        }),
        Event::SessionBeforeTree(prep) => serde_json::json!({
            "type": event_type,
            "target_id": prep.target_id,
            "old_leaf_id": prep.old_leaf_id,
        }),
        Event::SessionTree { new_leaf, old_leaf } => serde_json::json!({
            "type": event_type,
            "new_leaf": new_leaf,
            "old_leaf": old_leaf,
        }),
        Event::Context(ctx) => serde_json::json!({
            "type": event_type,
            "message_count": ctx.messages.len(),
        }),
        Event::BeforeProviderRequest { payload } => serde_json::json!({
            "type": event_type,
            "payload": payload,
        }),
        Event::Input(input) => serde_json::json!({
            "type": event_type,
            "text": input.text,
            "source": input.source,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_event_session_start() {
        let event = bb_hooks::Event::SessionStart;
        let json = serialize_event(&event);
        assert_eq!(json["type"], "session_start");
    }

    #[test]
    fn test_serialize_event_tool_call() {
        let event = bb_hooks::Event::ToolCall(bb_hooks::ToolCallEvent {
            tool_call_id: "tc1".into(),
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
        });
        let json = serialize_event(&event);
        assert_eq!(json["type"], "tool_call");
        assert_eq!(json["tool_name"], "bash");
        assert_eq!(json["input"]["command"], "ls");
    }
}
