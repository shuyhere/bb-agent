use super::*;

use crate::UsageInfo;

pub(super) fn process_google_event(event: &Value, tx: &mpsc::UnboundedSender<StreamEvent>) {
    if let Some(usage) = event.get("usageMetadata") {
        let input = usage["promptTokenCount"].as_u64().unwrap_or(0);
        let output = usage["candidatesTokenCount"].as_u64().unwrap_or(0);
        let cache_read = usage["cachedContentTokenCount"].as_u64().unwrap_or(0);
        let _ = tx.send(StreamEvent::Usage(UsageInfo {
            input_tokens: input.saturating_sub(cache_read),
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: 0,
        }));
    }

    let candidates = match event.get("candidates").and_then(|c| c.as_array()) {
        Some(c) => c,
        None => return,
    };

    for candidate in candidates {
        let parts = match candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
        {
            Some(p) => p,
            None => continue,
        };

        for part in parts {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                let _ = tx.send(StreamEvent::TextDelta {
                    text: text.to_string(),
                });
            }

            if let Some(fc) = part.get("functionCall") {
                let name = fc["name"].as_str().unwrap_or("").to_string();
                let args = fc.get("args").cloned().unwrap_or(json!({}));
                let id = format!("call_{}", name);
                let _ = tx.send(StreamEvent::ToolCallStart {
                    id: id.clone(),
                    name,
                });
                let _ = tx.send(StreamEvent::ToolCallDelta {
                    id: id.clone(),
                    arguments_delta: args.to_string(),
                });
                let _ = tx.send(StreamEvent::ToolCallEnd { id });
            }
        }
    }
}
