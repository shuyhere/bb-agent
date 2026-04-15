/// Utility: collect streaming events into final text + tool calls.
use bb_core::types::CacheMetricsSource;

use crate::StreamEvent;

pub struct CollectedResponse {
    pub text: String,
    pub thinking: String,
    pub tool_calls: Vec<CollectedToolCall>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cache_metrics_source: CacheMetricsSource,
}

pub struct CollectedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl CollectedResponse {
    pub fn from_events(events: &[StreamEvent]) -> Self {
        let mut text = String::new();
        let mut thinking = String::new();
        let mut tool_calls: Vec<CollectedToolCall> = Vec::new();
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut cache_read_tokens = 0u64;
        let mut cache_write_tokens = 0u64;
        let mut cache_metrics_source = CacheMetricsSource::Unknown;

        for event in events {
            match event {
                StreamEvent::TextDelta { text: t } => text.push_str(t),
                StreamEvent::ThinkingDelta { text: t } => thinking.push_str(t),
                StreamEvent::ToolCallStart { id, name } => {
                    tool_calls.push(CollectedToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: String::new(),
                    });
                }
                StreamEvent::ToolCallDelta {
                    id,
                    arguments_delta,
                } => {
                    if let Some(tc) = tool_calls.iter_mut().find(|tc| tc.id == *id) {
                        tc.arguments.push_str(arguments_delta);
                    }
                }
                StreamEvent::Usage(u) => {
                    input_tokens = u.input_tokens;
                    output_tokens = u.output_tokens;
                    cache_read_tokens = u.cache_read_tokens;
                    cache_write_tokens = u.cache_write_tokens;
                    cache_metrics_source = u.cache_metrics_source.clone();
                }
                _ => {}
            }
        }

        Self {
            text,
            thinking,
            tool_calls,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_write_tokens,
            cache_metrics_source,
        }
    }
}
