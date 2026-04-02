use std::time::Duration;

use bb_core::error::{BbError, BbResult};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use crate::types::{ProviderRetryEvent, RetryCallback};

pub async fn with_retry<F, Fut, T>(
    max_retries: u32,
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
                if attempt > 0 {
                    if let Some(callback) = &retry_callback {
                        callback(ProviderRetryEvent::End {
                            success: true,
                            attempt: used_attempts,
                            final_error: None,
                        });
                    }
                }
                return Ok(result);
            }
            Err(e) => {
                last_err = e;
                if attempt < max_retries - 1 {
                    let delay_ms = 1000 * 2u64.pow(attempt);
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
                            error_message: last_err.to_string(),
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

    let final_error = format!("Retry failed after {} attempts: {}", used_attempts, last_err);
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
        let result = with_retry(3, CancellationToken::new(), None, || {
            let c = c.clone();
            async move {
                let attempt = c.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    Err(BbError::Provider("temporary".into()))
                } else {
                    Ok(42)
                }
            }
        }).await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_all_fail() {
        let counter = Arc::new(AtomicU32::new(0));
        let c = counter.clone();
        let result: BbResult<i32> = with_retry(3, CancellationToken::new(), None, || {
            let c = c.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                Err(BbError::Provider("always fails".into()))
            }
        }).await;
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_succeeds_first_try() {
        let result = with_retry(3, CancellationToken::new(), None, || async {
            Ok::<_, BbError>(99)
        }).await;
        assert_eq!(result.unwrap(), 99);
    }
}
