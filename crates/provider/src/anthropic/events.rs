use serde_json::Value;
use tokio::sync::mpsc;

use crate::{CacheMetricsSource, StreamEvent, UsageInfo};

#[derive(Clone, Debug)]
enum BlockKind {
    ToolUse,
    ServerToolUse,
}

#[derive(Clone, Debug)]
struct TrackedBlock {
    id: String,
    kind: BlockKind,
}

/// Track block index → block metadata for correlating deltas.
static BLOCK_ID_MAP: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<u64, TrackedBlock>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

pub(super) fn process_sse_event(
    event: &Value,
    tx: &mpsc::UnboundedSender<StreamEvent>,
    cache_metrics_source: CacheMetricsSource,
) {
    let event_type = event["type"].as_str().unwrap_or("");

    match event_type {
        "message_start" => {
            BLOCK_ID_MAP
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clear();
            if let Some(usage) = event.get("message").and_then(|m| m.get("usage")) {
                let _ = tx.send(StreamEvent::Usage(UsageInfo {
                    input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
                    cache_read_tokens: usage["cache_read_input_tokens"].as_u64().unwrap_or(0),
                    cache_write_tokens: usage["cache_creation_input_tokens"].as_u64().unwrap_or(0),
                    cache_metrics_source: cache_metrics_source.clone(),
                }));
            }
        }
        "content_block_start" => {
            if let Some(block) = event.get("content_block") {
                let block_type = block["type"].as_str().unwrap_or("");
                match block_type {
                    "tool_use" => {
                        let id = block["id"].as_str().unwrap_or("").to_string();
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        let index = event["index"].as_u64().unwrap_or(0);
                        BLOCK_ID_MAP
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .insert(
                                index,
                                TrackedBlock {
                                    id: id.clone(),
                                    kind: BlockKind::ToolUse,
                                },
                            );
                        let _ = tx.send(StreamEvent::ToolCallStart { id, name });
                    }
                    "server_tool_use" => {
                        let id = block["id"].as_str().unwrap_or("").to_string();
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        let index = event["index"].as_u64().unwrap_or(0);
                        BLOCK_ID_MAP
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .insert(
                                index,
                                TrackedBlock {
                                    id: id.clone(),
                                    kind: BlockKind::ServerToolUse,
                                },
                            );
                        let _ = tx.send(StreamEvent::ServerToolUseStart { id, name });
                    }
                    t if t.ends_with("_tool_result") => {
                        let tool_use_id = block["tool_use_id"]
                            .as_str()
                            .or_else(|| block["source_tool_use_id"].as_str())
                            .or_else(|| block["id"].as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = t.trim_end_matches("_tool_result").to_string();
                        let is_error = block["is_error"].as_bool().unwrap_or(false)
                            || block["error"].is_object()
                            || block["error"].is_string()
                            || block["status"].as_str() == Some("error");
                        let _ = tx.send(StreamEvent::ServerToolResult {
                            tool_use_id,
                            name,
                            result: block.clone(),
                            is_error,
                        });
                    }
                    _ => {}
                }
            }
        }
        "content_block_delta" => {
            if let Some(delta) = event.get("delta") {
                let delta_type = delta["type"].as_str().unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(text) = delta["text"].as_str() {
                            let _ = tx.send(StreamEvent::TextDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta["thinking"].as_str() {
                            let _ = tx.send(StreamEvent::ThinkingDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                    "input_json_delta" => {
                        if let Some(json_str) = delta["partial_json"].as_str() {
                            let index = event["index"].as_u64().unwrap_or(0);
                            let tracked = BLOCK_ID_MAP
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .get(&index)
                                .cloned();
                            match tracked {
                                Some(TrackedBlock {
                                    id,
                                    kind: BlockKind::ToolUse,
                                }) => {
                                    let _ = tx.send(StreamEvent::ToolCallDelta {
                                        id,
                                        arguments_delta: json_str.to_string(),
                                    });
                                }
                                Some(TrackedBlock {
                                    id,
                                    kind: BlockKind::ServerToolUse,
                                }) => {
                                    let _ = tx.send(StreamEvent::ServerToolUseDelta {
                                        id,
                                        arguments_delta: json_str.to_string(),
                                    });
                                }
                                None => {
                                    let _ = tx.send(StreamEvent::ToolCallDelta {
                                        id: format!("block_{index}"),
                                        arguments_delta: json_str.to_string(),
                                    });
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        "content_block_stop" => {
            let index = event["index"].as_u64().unwrap_or(0);
            if let Some(tracked) = BLOCK_ID_MAP
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&index)
            {
                match tracked.kind {
                    BlockKind::ToolUse => {
                        let _ = tx.send(StreamEvent::ToolCallEnd { id: tracked.id });
                    }
                    BlockKind::ServerToolUse => {
                        let _ = tx.send(StreamEvent::ServerToolUseEnd { id: tracked.id });
                    }
                }
            }
        }
        "message_delta" => {
            if let Some(usage) = event.get("usage") {
                let _ = tx.send(StreamEvent::Usage(UsageInfo {
                    input_tokens: usage["input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: usage["output_tokens"].as_u64().unwrap_or(0),
                    cache_read_tokens: usage["cache_read_input_tokens"].as_u64().unwrap_or(0),
                    cache_write_tokens: usage["cache_creation_input_tokens"].as_u64().unwrap_or(0),
                    cache_metrics_source: cache_metrics_source.clone(),
                }));
            }
        }
        "message_stop" => {
            let _ = tx.send(StreamEvent::Done);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_server_tool_use_events() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        process_sse_event(
            &json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {
                    "type": "server_tool_use",
                    "id": "srv_1",
                    "name": "web_search"
                }
            }),
            &tx,
            CacheMetricsSource::Official,
        );
        process_sse_event(
            &json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": "{\"query\":\"rust async\"}"
                }
            }),
            &tx,
            CacheMetricsSource::Official,
        );
        process_sse_event(
            &json!({
                "type": "content_block_stop",
                "index": 0
            }),
            &tx,
            CacheMetricsSource::Official,
        );
        drop(tx);

        match rx.blocking_recv().expect("start") {
            StreamEvent::ServerToolUseStart { id, name } => {
                assert_eq!(id, "srv_1");
                assert_eq!(name, "web_search");
            }
            other => panic!("expected ServerToolUseStart, got {:?}", other),
        }
        match rx.blocking_recv().expect("delta") {
            StreamEvent::ServerToolUseDelta {
                id,
                arguments_delta,
            } => {
                assert_eq!(id, "srv_1");
                assert_eq!(arguments_delta, "{\"query\":\"rust async\"}");
            }
            other => panic!("expected ServerToolUseDelta, got {:?}", other),
        }
        match rx.blocking_recv().expect("stop") {
            StreamEvent::ServerToolUseEnd { id } => assert_eq!(id, "srv_1"),
            other => panic!("expected ServerToolUseEnd, got {:?}", other),
        }
    }

    #[test]
    fn parses_web_search_tool_result_blocks() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        process_sse_event(
            &json!({
                "type": "content_block_start",
                "index": 1,
                "content_block": {
                    "type": "web_search_tool_result",
                    "tool_use_id": "srv_1",
                    "content": [
                        { "title": "Tokio", "url": "https://tokio.rs" }
                    ]
                }
            }),
            &tx,
            CacheMetricsSource::Official,
        );
        drop(tx);

        match rx.blocking_recv().expect("result") {
            StreamEvent::ServerToolResult {
                tool_use_id,
                name,
                result,
                is_error,
            } => {
                assert_eq!(tool_use_id, "srv_1");
                assert_eq!(name, "web_search");
                assert!(!is_error);
                assert_eq!(result["content"][0]["url"], "https://tokio.rs");
            }
            other => panic!("expected ServerToolResult, got {:?}", other),
        }
    }

    #[test]
    fn marks_api_key_usage_as_official() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        process_sse_event(
            &json!({
                "type": "message_start",
                "message": {
                    "usage": {
                        "input_tokens": 100,
                        "output_tokens": 12,
                        "cache_read_input_tokens": 40,
                        "cache_creation_input_tokens": 5
                    }
                }
            }),
            &tx,
            CacheMetricsSource::Official,
        );
        drop(tx);

        match rx.blocking_recv().expect("usage event") {
            StreamEvent::Usage(usage) => {
                assert_eq!(usage.cache_metrics_source, CacheMetricsSource::Official);
                assert_eq!(usage.cache_read_tokens, 40);
                assert_eq!(usage.cache_write_tokens, 5);
            }
            other => panic!("expected Usage event, got {other:?}"),
        }
    }

    #[test]
    fn marks_oauth_usage_as_estimated() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        process_sse_event(
            &json!({
                "type": "message_delta",
                "usage": {
                    "input_tokens": 90,
                    "output_tokens": 8,
                    "cache_read_input_tokens": 30,
                    "cache_creation_input_tokens": 2
                }
            }),
            &tx,
            CacheMetricsSource::Estimated,
        );
        drop(tx);

        match rx.blocking_recv().expect("usage event") {
            StreamEvent::Usage(usage) => {
                assert_eq!(usage.cache_metrics_source, CacheMetricsSource::Estimated);
                assert_eq!(usage.cache_read_tokens, 30);
                assert_eq!(usage.cache_write_tokens, 2);
            }
            other => panic!("expected Usage event, got {other:?}"),
        }
    }
}
