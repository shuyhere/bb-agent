mod cache;
mod ddg;
mod input;

#[cfg(test)]
mod tests;

use async_trait::async_trait;
use bb_core::error::{BbError, BbResult};
use regex::Regex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

use crate::support::{emit_progress_line, text_result};
use crate::{Tool, ToolContext, ToolResult};

use cache::{build_cache_key, read_cached_search, write_cached_search};
#[cfg(test)]
use ddg::{apply_domain_filters, build_duckduckgo_query, is_bot_challenge, parse_duckduckgo_html};
use ddg::{format_output, normalize_domain, run_duckduckgo_search};
use input::validate_input;

const DDG_HTML_ENDPOINT: &str = "https://html.duckduckgo.com/html";
const DEFAULT_TIMEOUT_SECONDS: u64 = 20;
const DEFAULT_RESULT_COUNT: usize = 8;
const CACHE_TTL_SECONDS: u64 = 15 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchInput {
    pub query: String,
    pub allowed_domains: Option<Vec<String>>,
    pub blocked_domains: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum SearchChunk {
    Text {
        text: String,
    },
    Hits {
        tool_use_id: String,
        content: Vec<SearchHit>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebSearchOutput {
    pub query: String,
    pub results: Vec<SearchChunk>,
    pub duration_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawSearchResult {
    title: String,
    url: String,
    snippet: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedSearchValue {
    query: String,
    results: Vec<SearchChunk>,
    fetched_query: String,
    hit_count: usize,
}

#[derive(Debug, Clone)]
struct CachedSearchEntry {
    value: CachedSearchValue,
    expires_at: Instant,
}

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the public web using DuckDuckGo HTML results. Supports optional allowed_domains or blocked_domains filters, but not both at once. For research tasks, use this first to discover relevant pages, then use web_fetch on the most promising URLs before answering. Returns explicit source links."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to run"
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional allowlist of domains to prefer and keep"
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional blocklist of domains to exclude"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        params: Value,
        ctx: &ToolContext,
        cancel: CancellationToken,
    ) -> BbResult<ToolResult> {
        let input: WebSearchInput = serde_json::from_value(params)
            .map_err(|e| BbError::Tool(format!("Invalid web_search parameters: {e}")))?;
        validate_input(&input)?;

        let started = Instant::now();
        let cache_key = build_cache_key(&input);
        let cache_hit = if let Some(cached) = read_cached_search(&cache_key) {
            emit_progress_line(
                ctx,
                format!("Using cached DuckDuckGo results: {}", input.query.trim()),
            );
            let output = WebSearchOutput {
                query: cached.query.clone(),
                results: cached.results.clone(),
                duration_seconds: started.elapsed().as_secs_f64(),
            };
            let text = format_output(&output);
            return Ok(text_result(
                text,
                Some(json!({
                    "query": output.query,
                    "results": output.results,
                    "durationSeconds": output.duration_seconds,
                    "searchRequests": 0,
                    "backend": "duckduckgo-html",
                    "cacheHit": true,
                    "fetchedQuery": cached.fetched_query,
                    "usedAllowedDomains": input.allowed_domains.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
                    "usedBlockedDomains": input.blocked_domains.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
                    "hitCount": cached.hit_count,
                })),
            ));
        } else {
            emit_progress_line(ctx, format!("Searching DuckDuckGo: {}", input.query.trim()));
            false
        };

        let (output, fetched_query, hit_count) =
            run_duckduckgo_search(&input, cancel, started).await?;
        let text = format_output(&output);
        write_cached_search(
            cache_key,
            CachedSearchValue {
                query: output.query.clone(),
                results: output.results.clone(),
                fetched_query: fetched_query.clone(),
                hit_count,
            },
        );

        Ok(text_result(
            text,
            Some(json!({
                "query": output.query,
                "results": output.results,
                "durationSeconds": output.duration_seconds,
                "searchRequests": 1,
                "backend": "duckduckgo-html",
                "cacheHit": cache_hit,
                "fetchedQuery": fetched_query,
                "usedAllowedDomains": input.allowed_domains.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
                "usedBlockedDomains": input.blocked_domains.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
                "hitCount": hit_count,
            })),
        ))
    }
}
