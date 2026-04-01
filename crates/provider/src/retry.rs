use std::time::Duration;
use tokio::time::sleep;
use bb_core::error::{BbError, BbResult};

pub async fn with_retry<F, Fut, T>(max_retries: u32, f: F) -> BbResult<T>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = BbResult<T>>,
{
    let mut last_err = BbError::Provider("No attempts made".into());
    for attempt in 0..max_retries {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_err = e;
                if attempt < max_retries - 1 {
                    let delay = Duration::from_millis(1000 * 2u64.pow(attempt));
                    tracing::warn!("Provider request failed (attempt {}), retrying in {:?}", attempt + 1, delay);
                    sleep(delay).await;
                }
            }
        }
    }
    Err(last_err)
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
        let result = with_retry(3, || {
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
        let result: BbResult<i32> = with_retry(3, || {
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
        let result = with_retry(3, || async {
            Ok::<_, BbError>(99)
        }).await;
        assert_eq!(result.unwrap(), 99);
    }
}
