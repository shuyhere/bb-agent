use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::support::{emit_progress_line, text_result};
use crate::web::{parse_http_url, validate_optional_max_chars, validate_optional_timeout};
use crate::{Tool, ToolContext, ToolResult};

mod browser;
mod content;
#[cfg(test)]
mod tests;

use browser::{
    build_browser_args, create_temp_profile_dir, missing_browser_error_message,
    resolve_browser_executable, run_browser_dump_dom,
};
#[cfg(test)]
use content::extract_canonical_like_url;
use content::{is_browser_protection_page, resolve_final_url};

const DEFAULT_MAX_CHARS: usize = 20_000;
const MAX_MAX_CHARS: usize = 100_000;
const DEFAULT_TIMEOUT_SECONDS: f64 = 25.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrowserFetchInput {
    pub url: String,
    pub max_chars: Option<usize>,
    pub timeout: Option<f64>,
}

pub struct BrowserFetchTool;

#[async_trait]
impl Tool for BrowserFetchTool {
    fn name(&self) -> &str {
        "browser_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a page using a real local Chrome/Chromium browser in headless mode, then extract main content text. Use this when web_fetch is blocked by JavaScript, anti-bot pages, or authentication/cookie requirements. For web research tasks, usually use web_search first, then browser_fetch on the most promising protected or dynamic URLs, then summarize with explicit source links."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch in a real browser"
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
        let input: BrowserFetchInput = serde_json::from_value(params)
            .map_err(|e| BbError::Tool(format!("Invalid browser_fetch parameters: {e}")))?;
        validate_input(&input)?;

        let url = parse_http_url("browser_fetch", &input.url)?;

        let browser = resolve_browser_executable()
            .ok_or_else(|| BbError::Tool(missing_browser_error_message()))?;
        let timeout_secs = input.timeout.unwrap_or(DEFAULT_TIMEOUT_SECONDS).max(1.0);
        let max_chars = input
            .max_chars
            .unwrap_or(DEFAULT_MAX_CHARS)
            .min(MAX_MAX_CHARS);

        emit_progress_line(ctx, format!("Browser fetching: {url}"));

        let profile_dir = create_temp_profile_dir()?;
        let args = build_browser_args(&browser, url.as_ref(), &profile_dir, timeout_secs);

        let result = run_browser_dump_dom(&browser, &args, timeout_secs, cancel.clone()).await;
        let _ = tokio::fs::remove_dir_all(&profile_dir).await;
        let browser_output = result?;

        if browser_output.trim().is_empty() {
            return Err(BbError::Tool(
                "browser_fetch got no DOM output from the browser".into(),
            ));
        }

        if is_browser_protection_page(&browser_output) {
            return Err(BbError::Tool(
                "browser_fetch reached a protection/login/challenge page instead of readable article content".into(),
            ));
        }

        let final_url = resolve_final_url(url.as_str(), &browser_output).await;
        let title = crate::web_fetch::extract_title(&browser_output);
        let (extracted_text, extraction_source) =
            crate::web_fetch::extract_main_content_text(&browser_output);
        let truncated_text = crate::web_fetch::truncate_for_output(&extracted_text, max_chars);
        let was_truncated = extracted_text.chars().count() > truncated_text.chars().count();
        let citation_markdown =
            crate::web_fetch::build_citation_markdown(title.as_deref(), &final_url);
        let output = crate::web_fetch::build_web_fetch_output(
            "Browser Fetch",
            &final_url,
            title.as_deref(),
            "text/html",
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
                "contentType": "text/html",
                "title": title,
                "citationMarkdown": citation_markdown,
                "extractionSource": extraction_source,
                "maxChars": max_chars,
                "timeoutSeconds": timeout_secs,
                "truncated": was_truncated,
                "browserExecutable": browser.display().to_string(),
                "browserMode": "headless-dump-dom",
            })),
        ))
    }
}

fn validate_input(input: &BrowserFetchInput) -> BbResult<()> {
    if input.url.trim().is_empty() {
        return Err(BbError::Tool("browser_fetch url must be non-empty".into()));
    }
    validate_optional_max_chars("browser_fetch", input.max_chars)?;
    validate_optional_timeout("browser_fetch", input.timeout)?;
    Ok(())
}
