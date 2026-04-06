mod convert;
mod events;
#[cfg(test)]
mod tests;

use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::{CompletionRequest, Provider, RequestOptions, StreamEvent, retry::with_retry};

pub use convert::{convert_messages_google, convert_tools_google};
use events::process_google_event;

/// Google Generative AI (Gemini) provider.
pub struct GoogleProvider {
    client: Client,
}

impl Default for GoogleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleProvider {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl Provider for GoogleProvider {
    fn name(&self) -> &str {
        "google"
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
    ) -> BbResult<Vec<StreamEvent>> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.stream(request, options, tx).await?;

        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        Ok(events)
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        options: RequestOptions,
        tx: mpsc::UnboundedSender<StreamEvent>,
    ) -> BbResult<()> {
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?key={}&alt=sse",
            options.base_url.trim_end_matches('/'),
            request.model,
            options.api_key,
        );

        let contents = convert_messages_google(&request.messages);
        let tools = convert_tools_google(&request.tools);

        let mut body = json!({
            "contents": contents,
            "generationConfig": {
                "maxOutputTokens": request.max_tokens.unwrap_or(16384),
            }
        });

        if !request.system_prompt.is_empty() {
            body["systemInstruction"] = json!({
                "parts": [{ "text": request.system_prompt }]
            });
        }

        if !tools.is_empty() {
            body["tools"] = json!([{ "functionDeclarations": tools }]);
        }

        let response = with_retry(
            options.max_retries,
            options.retry_base_delay_ms,
            options.max_retry_delay_ms,
            options.cancel.clone(),
            options.retry_callback.clone(),
            || {
                let mut req = self
                    .client
                    .post(&url)
                    .header("content-type", "application/json");

                for (k, v) in &options.headers {
                    req = req.header(k.as_str(), v.as_str());
                }
                let body_clone = body.clone();
                async move {
                    let response = req
                        .json(&body_clone)
                        .send()
                        .await
                        .map_err(|e| BbError::Provider(format!("Request failed: {e}")))?;

                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        return Err(BbError::Provider(format!("HTTP {status}: {body}")));
                    }
                    Ok(response)
                }
            },
        )
        .await?;

        use futures::StreamExt;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            if options.cancel.is_cancelled() {
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }

            let chunk =
                chunk_result.map_err(|e| BbError::Provider(format!("Stream error: {e}")))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(pos) = buffer.find("\n\n") {
                let block = buffer[..pos].to_string();
                buffer = buffer[pos + 2..].to_string();

                for line in block.lines() {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            let _ = tx.send(StreamEvent::Done);
                            return Ok(());
                        }
                        if let Ok(event) = serde_json::from_str::<Value>(data) {
                            process_google_event(&event, &tx);
                        }
                    }
                }
            }
        }

        let _ = tx.send(StreamEvent::Done);
        Ok(())
    }
}
