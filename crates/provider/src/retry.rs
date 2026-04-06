use std::time::Duration;

use bb_core::error::{BbError, BbResult};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::types::{ProviderRetryEvent, RetryCallback};

fn is_retryable_error_message(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    [
        "overloaded",
        "provider returned error",
        "rate limit",
        "too many requests",
        "resource exhausted",
        "429",
        "500",
        "502",
        "503",
        "504",
        "service unavailable",
        "server error",
        "internal error",
        "network error",
        "connection error",
        "connection refused",
        "other side closed",
        "fetch failed",
        "upstream connect",
        "reset before headers",
        "socket hang up",
        "timed out",
        "timeout",
        "terminated",
        "retry delay",
    ]
    .iter()
    .any(|needle| msg.contains(needle))
}

fn extract_retry_delay_ms(message: &str) -> Option<u64> {
    let normalize = |ms: f64| -> Option<u64> {
        if ms.is_finite() && ms > 0.0 {
            Some((ms.ceil() as u64).saturating_add(1_000))
        } else {
            None
        }
    };

    let lower = message.to_ascii_lowercase();

    if let Some(caps) = regex::Regex::new(r"please retry in ([0-9.]+)(ms|s)")
        .ok()?
        .captures(&lower)
    {
        let value = caps.get(1)?.as_str().parse::<f64>().ok()?;
        let unit = caps.get(2)?.as_str();
        let ms = if unit == "ms" { value } else { value * 1000.0 };
        return normalize(ms);
    }

    if let Some(caps) = regex::Regex::new(r#"retrydelay"\s*:\s*"([0-9.]+)(ms|s)"#)
        .ok()?
        .captures(&lower)
    {
        let value = caps.get(1)?.as_str().parse::<f64>().ok()?;
        let unit = caps.get(2)?.as_str();
        let ms = if unit == "ms" { value } else { value * 1000.0 };
        return normalize(ms);
    }

    if let Some(caps) = regex::Regex::new(r"reset after (?:(\d+)h)?(?:(\d+)m)?(\d+(?:\.\d+)?)s")
        .ok()?
        .captures(&lower)
    {
        let hours = caps
            .get(1)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let mins = caps
            .get(2)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        let secs = caps
            .get(3)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .unwrap_or(0.0);
        return normalize(((hours * 60.0 + mins) * 60.0 + secs) * 1000.0);
    }

    None
}

pub async fn with_retry<F, Fut, T>(
    max_retries: u32,
    base_delay_ms: u64,
    max_retry_delay_ms: u64,
    cancel: CancellationToken,
    retry_callback: Option<RetryCallback>,
    f: F,
) -> BbResult<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = BbResult<T>>,
{
    let mut last_err = BbError::Provider("No attempts made".into());
    let mut used_attempts = 0_u32;

    for attempt in 0..max_retries {
        used_attempts = attempt + 1;
        match f().await {
            Ok(result) => {
                if attempt > 0
                    && let Some(callback) = &retry_callback
                {
                    callback(ProviderRetryEvent::End {
                        success: true,
                        attempt: used_attempts,
                        final_error: None,
                    });
                }
                return Ok(result);
            }
            Err(e) => {
                last_err = e;
                let last_message = last_err.to_string();
                let retryable = is_retryable_error_message(&last_message);
                if !retryable {
                    return Err(last_err);
                }
                if attempt < max_retries - 1 {
                    let server_delay_ms = extract_retry_delay_ms(&last_message);
                    if let Some(server_delay_ms) = server_delay_ms
                        && max_retry_delay_ms > 0
                        && server_delay_ms > max_retry_delay_ms
                    {
                        let final_error = format!(
                            "Server requested {}s retry delay (max: {}s). {}",
                            server_delay_ms.div_ceil(1000),
                            max_retry_delay_ms.div_ceil(1000),
                            last_message
                        );
                        if let Some(callback) = &retry_callback {
                            callback(ProviderRetryEvent::End {
                                success: false,
                                attempt: used_attempts,
                                final_error: Some(final_error.clone()),
                            });
                        }
                        return Err(BbError::Provider(final_error));
                    }
                    let delay_ms = server_delay_ms
                        .unwrap_or_else(|| base_delay_ms.saturating_mul(2u64.pow(attempt)));
                    let delay = Duration::from_millis(delay_ms);
                    tracing::warn!(
                        "Provider request failed (attempt {}), retrying in {:?}",
                        used_attempts,
                        delay
                    );
                    if let Some(callback) = &retry_callback {
                        callback(ProviderRetryEvent::Start {
                            attempt: used_attempts,
                            max_attempts: max_retries,
                            delay_ms,
                            error_message: last_message,
                        });
                    }
                    tokio::select! {
                        _ = sleep(delay) => {}
                        _ = cancel.cancelled() => {
                            if let Some(callback) = &retry_callback {
                                callback(ProviderRetryEvent::End {
                                    success: false,
                                    attempt: used_attempts,
                                    final_error: Some("Retry cancelled".to_string()),
                                });
                            }
                            return Err(BbError::Provider("Retry cancelled".into()));
                        }
                    }
                }
            }
        }
    }

    let final_error = format!(
        "Retry failed after {} attempts: {}",
        used_attempts, last_err
    );
    if let Some(callback) = &retry_callback {
        callback(ProviderRetryEvent::End {
            success: false,
            attempt: used_attempts,
            final_error: Some(final_error.clone()),
        });
    }
    Err(BbError::Provider(final_error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_retry_succeeds_on_second_attempt() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        let result = with_retry(3, 1_000, 60_000, CancellationToken::new(), None, || {
            let c = c.clone();
            async move {
                let attempt = c.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    Err(BbError::Provider("HTTP 429: temporary".into()))
                } else {
                    Ok(42)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_all_fail() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        let result: BbResult<i32> =
            with_retry(3, 1_000, 60_000, CancellationToken::new(), None, || {
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err(BbError::Provider("HTTP 429: always fails".into()))
                }
            })
            .await;
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_non_retryable_error_stops_immediately() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        let result: BbResult<i32> =
            with_retry(3, 1_000, 60_000, CancellationToken::new(), None, || {
                let c = c.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Err(BbError::Provider("HTTP 401: unauthorized".into()))
                }
            })
            .await;
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_retry_succeeds_first_try() {
        let result = with_retry(3, 1_000, 60_000, CancellationToken::new(), None, || async {
            Ok::<_, BbError>(99)
        })
        .await;
        assert_eq!(result.unwrap(), 99);
    }
}
