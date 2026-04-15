use anyhow::Result;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use super::tracker::RequestCacheMetrics;

static REQUEST_METRICS_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub fn append_request_metrics_jsonl(path: &Path, metrics: &RequestCacheMetrics) -> Result<()> {
    let _guard = REQUEST_METRICS_FILE_LOCK
        .lock()
        .expect("request metrics file lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    write_request_metrics_jsonl(&mut file, metrics)
}

pub fn write_request_metrics_jsonl<W: Write>(
    writer: &mut W,
    metrics: &RequestCacheMetrics,
) -> Result<()> {
    serde_json::to_writer(&mut *writer, metrics)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn latest_request_metrics_for_session(
    path: &Path,
    session_id: &str,
) -> Result<Option<RequestCacheMetrics>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };

    let mut latest = None;
    for line in BufReader::new(file).lines() {
        let line = line?;
        let Ok(metrics) = serde_json::from_str::<RequestCacheMetrics>(&line) else {
            continue;
        };
        if metrics.session_id == session_id {
            latest = Some(metrics);
        }
    }

    Ok(latest)
}

#[cfg(test)]
mod tests {
    use super::{append_request_metrics_jsonl, latest_request_metrics_for_session};
    use crate::cache_metrics::CacheMetricsSource;
    use crate::request_metrics::RequestCacheMetrics;
    use std::fs;

    #[test]
    fn appends_jsonl_metrics_records() {
        let path = std::env::temp_dir().join(format!(
            "bb-monitor-request-metrics-{}.jsonl",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        let metrics = RequestCacheMetrics {
            request_id: "req-1".to_string(),
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            turn_index: 1,
            context_epoch: 0,
            stable_prefix_hash: "stable".to_string(),
            stable_prefix_bytes: 10,
            full_request_hash: "full".to_string(),
            provider_messages_hash: "messages".to_string(),
            tool_defs_hash: "tools".to_string(),
            system_prompt_hash: "system".to_string(),
            previous_request_hash: None,
            first_divergence_byte: None,
            first_divergence_token_estimate: None,
            reused_prefix_bytes_estimate: None,
            reused_prefix_tokens_estimate: None,
            prompt_bytes: 42,
            message_count: 1,
            tool_count: 0,
            cache_metrics_source: CacheMetricsSource::Unknown,
            provider_cache_read_tokens: Some(0),
            provider_cache_write_tokens: Some(0),
            estimated_cache_read_tokens: None,
            estimated_cache_write_tokens: None,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            input_tokens: 12,
            output_tokens: 4,
            prompt_token_total: 12,
            cache_read_hit_rate_pct: None,
            cache_effective_utilization_pct: None,
            warm_request: false,
            request_started_at_ms: 10,
            first_stream_event_at_ms: Some(11),
            first_text_delta_at_ms: Some(12),
            finished_at_ms: 20,
            ttft_ms: Some(2),
            total_latency_ms: 10,
            tool_wait_ms: 0,
            resume_latency_ms: None,
            post_compaction: false,
            system_prompt_mutated: false,
            context_rewritten: false,
            request_rewritten: false,
        };

        append_request_metrics_jsonl(&path, &metrics).expect("append metrics");
        let written = fs::read_to_string(&path).expect("read metrics log");
        assert!(written.contains("\"request_id\":\"req-1\""));
        assert!(written.ends_with('\n'));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn reads_latest_metrics_for_matching_session() {
        let path = std::env::temp_dir().join(format!(
            "bb-monitor-request-metrics-read-{}.jsonl",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        let first = RequestCacheMetrics {
            request_id: "req-1".to_string(),
            session_id: "session-1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-5".to_string(),
            turn_index: 1,
            context_epoch: 0,
            stable_prefix_hash: "stable".to_string(),
            stable_prefix_bytes: 10,
            full_request_hash: "full".to_string(),
            provider_messages_hash: "messages".to_string(),
            tool_defs_hash: "tools".to_string(),
            system_prompt_hash: "system".to_string(),
            previous_request_hash: None,
            first_divergence_byte: None,
            first_divergence_token_estimate: None,
            reused_prefix_bytes_estimate: None,
            reused_prefix_tokens_estimate: None,
            prompt_bytes: 42,
            message_count: 1,
            tool_count: 0,
            cache_metrics_source: CacheMetricsSource::Official,
            provider_cache_read_tokens: Some(10),
            provider_cache_write_tokens: Some(0),
            estimated_cache_read_tokens: None,
            estimated_cache_write_tokens: None,
            cache_read_tokens: 10,
            cache_write_tokens: 0,
            input_tokens: 90,
            output_tokens: 4,
            prompt_token_total: 100,
            cache_read_hit_rate_pct: Some(10.0),
            cache_effective_utilization_pct: Some(10.0),
            warm_request: true,
            request_started_at_ms: 10,
            first_stream_event_at_ms: Some(11),
            first_text_delta_at_ms: Some(12),
            finished_at_ms: 20,
            ttft_ms: Some(2),
            total_latency_ms: 10,
            tool_wait_ms: 0,
            resume_latency_ms: None,
            post_compaction: false,
            system_prompt_mutated: false,
            context_rewritten: false,
            request_rewritten: false,
        };
        let mut second = first.clone();
        second.request_id = "req-2".to_string();
        second.turn_index = 2;
        second.cache_read_hit_rate_pct = Some(20.0);
        second.cache_read_tokens = 20;
        let mut other = first.clone();
        other.request_id = "req-x".to_string();
        other.session_id = "session-2".to_string();

        append_request_metrics_jsonl(&path, &first).expect("append first");
        append_request_metrics_jsonl(&path, &other).expect("append other");
        append_request_metrics_jsonl(&path, &second).expect("append second");

        let latest = latest_request_metrics_for_session(&path, "session-1")
            .expect("read latest")
            .expect("matching metrics");
        assert_eq!(latest.request_id, "req-2");
        assert_eq!(latest.turn_index, 2);
        assert_eq!(latest.cache_read_hit_rate_pct, Some(20.0));

        let _ = fs::remove_file(&path);
    }
}
