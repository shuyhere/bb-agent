use anyhow::{Context, Result};
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

/// Result received from the browser redirect.
#[derive(Debug, Clone)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

/// Handle to a running callback server.
pub struct CallbackServer {
    /// Resolves once the browser hits the callback URL.
    pub result_rx: oneshot::Receiver<Result<CallbackParams>>,
    /// Drop or send to cancel the listener task.
    cancel_tx: oneshot::Sender<()>,
}

/// Destructured parts of a `CallbackServer`, useful for `tokio::select!`.
pub struct CallbackServerParts {
    pub result_rx: oneshot::Receiver<Result<CallbackParams>>,
    pub cancel_tx: oneshot::Sender<()>,
}

impl CallbackServer {
    /// Cancel the background listener.
    pub fn cancel(self) {
        let _ = self.cancel_tx.send(());
    }

    /// Destructure into individual fields so they can be used in
    /// `tokio::select!` without partial-move issues.
    pub fn into_parts(self) -> CallbackServerParts {
        CallbackServerParts {
            result_rx: self.result_rx,
            cancel_tx: self.cancel_tx,
        }
    }
}

/// Start a one-shot HTTP server on `127.0.0.1:{port}` that waits for a GET
/// to `expected_path` (e.g. `/callback`) carrying `code` and `state` query
/// parameters.
pub async fn start_callback_server(port: u16, expected_path: &str) -> Result<CallbackServer> {
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .with_context(|| format!("Failed to bind callback server on port {port}"))?;

    let (result_tx, result_rx) = oneshot::channel();
    let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
    let path = expected_path.to_string();

    tokio::spawn(async move {
        tokio::select! {
            _ = cancel_rx => {}
            accepted = listener.accept() => {
                let result = match accepted {
                    Ok((stream, _addr)) => handle_connection(stream, &path).await,
                    Err(e) => Err(anyhow::anyhow!("Accept failed: {e}")),
                };
                let _ = result_tx.send(result);
            }
        }
    });

    Ok(CallbackServer {
        result_rx,
        cancel_tx,
    })
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    expected_path: &str,
) -> Result<CallbackParams> {
    let mut buf = vec![0u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .context("Failed to read from callback connection")?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the request line: "GET /callback?code=...&state=... HTTP/1.1"
    let request_line = request.lines().next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();

    if parts.len() < 2 || parts[0] != "GET" {
        send_response(&mut stream, 400, "Bad Request", "Expected GET request").await;
        anyhow::bail!("Not a GET request: {request_line}");
    }

    let full_path = parts[1];
    let (path_part, query_part) = full_path.split_once('?').unwrap_or((full_path, ""));

    if path_part != expected_path {
        send_response(
            &mut stream,
            404,
            "Not Found",
            &format!("Unexpected path: {path_part}"),
        )
        .await;
        anyhow::bail!("Unexpected callback path: {path_part}");
    }

    let params = parse_query(query_part);

    let code = params
        .get("code")
        .cloned()
        .unwrap_or_default();
    let state = params
        .get("state")
        .cloned()
        .unwrap_or_default();

    if code.is_empty() {
        // Check for error
        let error = params.get("error").cloned().unwrap_or_default();
        let desc = params
            .get("error_description")
            .cloned()
            .unwrap_or_else(|| "Unknown error".into());
        let msg = if error.is_empty() {
            "Missing 'code' parameter".to_string()
        } else {
            format!("{error}: {desc}")
        };
        send_response(&mut stream, 400, "Authentication Failed", &msg).await;
        anyhow::bail!("OAuth callback error: {msg}");
    }

    send_response(
        &mut stream,
        200,
        "Authentication Successful",
        "You can close this tab and return to the terminal.",
    )
    .await;

    Ok(CallbackParams { code, state })
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            Some((
                url_decode(k),
                url_decode(v),
            ))
        })
        .collect()
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        match b {
            b'%' => {
                let hi = chars.next().unwrap_or(b'0');
                let lo = chars.next().unwrap_or(b'0');
                let hex = [hi, lo];
                if let Ok(s) = std::str::from_utf8(&hex) {
                    if let Ok(val) = u8::from_str_radix(s, 16) {
                        result.push(val as char);
                        continue;
                    }
                }
                result.push('%');
                result.push(hi as char);
                result.push(lo as char);
            }
            b'+' => result.push(' '),
            _ => result.push(b as char),
        }
    }
    result
}

async fn send_response(stream: &mut tokio::net::TcpStream, status: u16, title: &str, body: &str) {
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head><title>{title}</title></head>
<body style="font-family:system-ui,sans-serif;display:flex;justify-content:center;align-items:center;min-height:80vh">
<div style="text-align:center">
<h1>{title}</h1>
<p>{body}</p>
</div>
</body>
</html>"#,
    );
    let response = format!(
        "HTTP/1.1 {status} {title}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {html}",
        html.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}
