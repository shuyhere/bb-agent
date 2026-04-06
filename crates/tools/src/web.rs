use bb_core::error::{BbError, BbResult};
use reqwest::{Client, RequestBuilder, Response, Url};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub(crate) const STANDARD_WEB_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

pub(crate) fn parse_http_url(tool_name: &str, raw_url: &str) -> BbResult<Url> {
    let url = Url::parse(raw_url.trim()).map_err(|e| BbError::Tool(format!("Invalid URL: {e}")))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(BbError::Tool(format!(
            "{tool_name} only supports http/https URLs, got {other}"
        ))),
    }
}

pub(crate) fn validate_optional_max_chars(
    tool_name: &str,
    max_chars: Option<usize>,
) -> BbResult<()> {
    if let Some(max_chars) = max_chars
        && max_chars == 0
    {
        return Err(BbError::Tool(format!("{tool_name} max_chars must be > 0")));
    }
    Ok(())
}

pub(crate) fn validate_optional_timeout(tool_name: &str, timeout: Option<f64>) -> BbResult<()> {
    if let Some(timeout) = timeout
        && (!timeout.is_finite() || timeout <= 0.0)
    {
        return Err(BbError::Tool(format!("{tool_name} timeout must be > 0")));
    }
    Ok(())
}

pub(crate) fn create_web_client(
    client_label: &str,
    timeout: Duration,
    max_redirects: usize,
) -> BbResult<Client> {
    Client::builder()
        .user_agent(STANDARD_WEB_USER_AGENT)
        .redirect(reqwest::redirect::Policy::limited(max_redirects))
        .timeout(timeout)
        .build()
        .map_err(|e| BbError::Tool(format!("Failed to create {client_label} client: {e}")))
}

pub(crate) async fn send_with_cancel(
    request: RequestBuilder,
    cancel: &CancellationToken,
    cancelled_message: &'static str,
    request_error_prefix: &'static str,
) -> BbResult<Response> {
    tokio::select! {
        _ = cancel.cancelled() => Err(BbError::Tool(cancelled_message.into())),
        response = request.send() => {
            response.map_err(|e| BbError::Tool(format!("{request_error_prefix}: {e}")))
        }
    }
}

pub(crate) async fn read_text_with_cancel(
    response: Response,
    cancel: &CancellationToken,
    cancelled_message: &'static str,
    read_error_prefix: &'static str,
) -> BbResult<String> {
    tokio::select! {
        _ = cancel.cancelled() => Err(BbError::Tool(cancelled_message.into())),
        text = response.text() => {
            text.map_err(|e| BbError::Tool(format!("{read_error_prefix}: {e}")))
        }
    }
}
