mod html;
mod output;

use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{sync::LazyLock, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::support::{emit_progress_line, text_result};
use crate::web::{
    create_web_client, parse_http_url, read_text_with_cancel, send_with_cancel,
    validate_optional_max_chars, validate_optional_timeout,
};
use crate::{Tool, ToolContext, ToolResult};

pub(crate) use html::{extract_main_content_text, extract_title};
pub(crate) use output::{build_citation_markdown, build_web_fetch_output, truncate_for_output};

const DEFAULT_MAX_CHARS: usize = 20_000;
const MAX_MAX_CHARS: usize = 100_000;
const DEFAULT_TIMEOUT_SECONDS: f64 = 20.0;
const MAX_REDIRECTS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebFetchInput {
    pub url: String,
    pub max_chars: Option<usize>,
    pub timeout: Option<f64>,
}

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a public web page by URL and return extracted main content text. For web research tasks, usually use web_search first, then web_fetch 1-3 promising result pages, then summarize with explicit source links. External web content is treated as untrusted."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "max_chars": {
                    "type": "number",
                    "description": "Maximum number of characters to return (default 20000, max 100000)"
                },
                "timeout": {
                    "type": "number",
                    "description": "Optional timeout in seconds"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let input: WebFetchInput = serde_json::from_value(params)
            .map_err(|e| BbError::Tool(format!("Invalid web_fetch parameters: {e}")))?;
        validate_input(&input)?;

        let url = parse_http_url("web_fetch", &input.url)?;

        let timeout_secs = input.timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS).max(1.0);
        let max_chars = input
            .max_chars
            .unwrap_or(DEFAULT_MAX_CHARS)
            .min(MAX_MAX_CHARS);

        emit_progress_line(ctx, format!("Fetching: {url}"));

        let client = create_web_client(
            "web_fetch",
            Duration::from_secs_f64(timeout_secs),
            MAX_REDIRECTS,
        )?;

        let response = send_with_cancel(
            client.get(url.clone()),
            &cancel,
            "Web fetch cancelled",
            "Web fetch request failed",
        )
        .await?;

        let final_url = response.url().to_string();
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = read_text_with_cancel(
            response,
            &cancel,
            "Web fetch cancelled",
            "Failed to read web_fetch response",
        )
        .await?;

        if !status.is_success() {
            return Err(BbError::Tool(format!(
                "Web fetch failed ({}): {}",
                status,
                truncate_for_output(&body, 800)
            )));
        }

        let title = extract_title(&body);
        let (extracted_text, extraction_source) =
            if content_type.contains("html") || body.contains("<html") || body.contains("<body") {
                extract_main_content_text(&body)
            } else {
                (body, "plain_text")
            };
        let truncated_text = truncate_for_output(&extracted_text, max_chars);
        let was_truncated = extracted_text.chars().count() > truncated_text.chars().count();
        let citation_markdown = build_citation_markdown(title.as_deref(), &final_url);
        let output = build_web_fetch_output(
            "Web Fetch",
            &final_url,
            title.as_deref(),
            &content_type,
            extraction_source,
            max_chars,
            was_truncated,
            &truncated_text,
            &citation_markdown,
        );

        Ok(text_result(
            output,
            Some(json!({
                "url": input.url,
                "finalUrl": final_url,
                "contentType": content_type,
                "title": title,
                "citationMarkdown": citation_markdown,
                "extractionSource": extraction_source,
                "maxChars": max_chars,
                "timeoutSeconds": timeout_secs,
                "truncated": was_truncated,
                "status": status.as_u16(),
            })),
        ))
    }
}

fn validate_input(input: &WebFetchInput) -> BbResult<()> {
    if input.url.trim().is_empty() {
        return Err(BbError::Tool("web_fetch url must be non-empty".into()));
    }
    validate_optional_max_chars("web_fetch", input.max_chars)?;
    validate_optional_timeout("web_fetch", input.timeout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bb_core::types::ContentBlock;
    use std::io::{Read, Write};
    use std::path::Path;
    use std::thread;

    fn make_ctx(dir: &Path) -> ToolContext {
        ToolContext {
            cwd: dir.to_path_buf(),
            artifacts_dir: dir.to_path_buf(),
            execution_policy: crate::ExecutionPolicy::Safety,
            on_output: None,
            web_search: None,
            execution_mode: crate::ToolExecutionMode::Interactive,
            request_approval: None,
        }
    }

    fn spawn_single_response_server(
        status_line: &str,
        content_type: &str,
        body: &str,
    ) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("local addr");
        let body = body.to_string();
        let content_type = content_type.to_string();
        let status_line = status_line.to_string();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
            let mut request_buf = [0u8; 2048];
            let _ = stream.read(&mut request_buf);
            let response = format!(
                "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            let _ = stream.flush();
        });

        format!("http://{addr}/github-contents")
    }

    #[test]
    fn validation_fails_on_empty_url() {
        let err = validate_input(&WebFetchInput {
            url: "   ".into(),
            max_chars: None,
            timeout: None,
        })
        .unwrap_err();
        assert!(err.to_string().contains("non-empty"));
    }

    #[test]
    fn extracts_title_and_visible_text_from_html() {
        let html = r#"
            <html>
              <head><title>Example Page</title><style>.x { color: red; }</style></head>
              <body>
                <script>ignore()</script>
                <h1>Hello</h1>
                <p>World &amp; friends</p>
              </body>
            </html>
        "#;
        assert_eq!(extract_title(html).as_deref(), Some("Example Page"));
        let (text, source) = extract_main_content_text(html);
        assert_eq!(source, "body");
        assert!(text.contains("Hello"));
        assert!(text.contains("World & friends"));
        assert!(!text.contains("ignore()"));
    }

    #[test]
    fn prefers_main_content_over_navigation_and_footer() {
        let html = r#"
            <html>
              <body>
                <nav>
                  <a href="/a">Home</a>
                  <a href="/b">Pricing</a>
                </nav>
                <main>
                  <article>
                    <h1>Real Article</h1>
                    <p>First paragraph.</p>
                    <p>Second paragraph with substance.</p>
                  </article>
                </main>
                <footer>copyright and boilerplate</footer>
              </body>
            </html>
        "#;
        let (text, source) = extract_main_content_text(html);
        assert_eq!(source, "main");
        assert!(text.contains("Real Article"));
        assert!(text.contains("First paragraph."));
        assert!(text.contains("Second paragraph with substance."));
        assert!(!text.contains("Pricing"));
        assert!(!text.contains("copyright"));
    }

    #[test]
    fn preserves_paragraph_breaks_for_readability() {
        let html = r#"
            <main>
              <h1>Doc</h1>
              <p>Paragraph one.</p>
              <p>Paragraph two.</p>
              <ul><li>Item A</li><li>Item B</li></ul>
            </main>
        "#;
        let (text, _) = extract_main_content_text(html);
        assert!(text.contains("Paragraph one.\n\nParagraph two."));
        assert!(text.contains("- Item A"));
        assert!(text.contains("- Item B"));
    }

    #[test]
    fn truncates_text_at_char_limit() {
        let text = truncate_for_output("abcdef", 3);
        assert_eq!(text, "abc");
    }

    #[test]
    fn citation_markdown_uses_exact_final_url() {
        let citation = build_citation_markdown(
            Some("Tokio Shutdown"),
            "https://tokio.rs/tokio/topics/shutdown",
        );
        assert_eq!(
            citation,
            "- [Tokio Shutdown](https://tokio.rs/tokio/topics/shutdown)"
        );
    }

    #[test]
    fn output_ends_with_explicit_citation_block() {
        let output = build_web_fetch_output(
            "Web Fetch",
            "https://tokio.rs/tokio/topics/shutdown",
            Some("Tokio Shutdown"),
            "text/html",
            "main",
            20_000,
            false,
            "Important fetched content.",
            "- [Tokio Shutdown](https://tokio.rs/tokio/topics/shutdown)",
        );
        assert!(
            output.contains(
                "\n\nCitation:\n- [Tokio Shutdown](https://tokio.rs/tokio/topics/shutdown)"
            )
        );
        assert!(output.contains("copy the citation line above exactly"));
    }

    #[tokio::test]
    async fn public_json_api_response_is_returned_as_plain_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let url = spawn_single_response_server(
            "200 OK",
            "application/json; charset=utf-8",
            r#"[{"name":"cli","path":"codex-rs/cli","type":"dir"}]"#,
        );

        let result = WebFetchTool
            .execute(
                serde_json::json!({
                    "url": url,
                    "max_chars": 2_000,
                    "timeout": 5,
                }),
                &make_ctx(dir.path()),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .expect("web_fetch should accept public JSON API responses");

        assert!(!result.is_error);
        let details = result.details.expect("details");
        assert_eq!(
            details.get("contentType").and_then(|v| v.as_str()),
            Some("application/json; charset=utf-8")
        );
        assert_eq!(
            details.get("extractionSource").and_then(|v| v.as_str()),
            Some("plain_text")
        );
        assert_eq!(details.get("status").and_then(|v| v.as_u64()), Some(200));

        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
            _ => panic!("expected text result"),
        };
        assert!(text.contains("\"name\":\"cli\""));
        assert!(text.contains("\"path\":\"codex-rs/cli\""));
        assert!(text.contains("Content-Type: application/json; charset=utf-8"));
        assert!(text.contains("Extraction: plain_text"));
    }
}
