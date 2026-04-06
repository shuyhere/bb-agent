use futures::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Once;

tokio::task_local! {
    static SUPPRESS_CONTAINED_PANIC_HOOK: bool;
}

fn install_contained_panic_hook() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let suppressed = SUPPRESS_CONTAINED_PANIC_HOOK
                .try_with(|flag| *flag)
                .unwrap_or(false);
            if !suppressed {
                previous(info);
            }
        }));
    });
}

pub(super) async fn catch_contained_panics<T, F>(future: F) -> std::result::Result<T, String>
where
    F: Future<Output = T>,
{
    install_contained_panic_hook();
    SUPPRESS_CONTAINED_PANIC_HOOK
        .scope(true, AssertUnwindSafe(future).catch_unwind())
        .await
        .map_err(|payload| panic_payload_to_string(payload.as_ref()))
}

fn panic_payload_to_string(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}
