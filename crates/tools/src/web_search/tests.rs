use super::*;
use serde_json::json;

#[test]
fn validation_fails_on_empty_query() {
    let err = super::input::validate_input(&WebSearchInput {
        query: "   ".into(),
        allowed_domains: None,
        blocked_domains: None,
    })
    .unwrap_err();
    assert!(err.to_string().contains("non-empty"));
}

#[test]
fn validation_fails_when_both_filters_are_set() {
    let err = super::input::validate_input(&WebSearchInput {
        query: "rust".into(),
        allowed_domains: Some(vec!["docs.rs".into()]),
        blocked_domains: Some(vec!["example.com".into()]),
    })
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("both allowed_domains and blocked_domains")
    );
}

#[test]
fn validation_allows_empty_blocked_domains_with_allowlist() {
    super::input::validate_input(&WebSearchInput {
        query: "rust".into(),
        allowed_domains: Some(vec!["docs.rs".into()]),
        blocked_domains: Some(vec![]),
    })
    .expect("empty blocked_domains should be treated as absent");
}

#[test]
fn duckduckgo_query_includes_domain_filters() {
    let query = build_duckduckgo_query(&WebSearchInput {
        query: "rust async cancellation".into(),
        allowed_domains: Some(vec!["docs.rs".into(), "tokio.rs".into()]),
        blocked_domains: None,
    });
    assert!(query.contains("site:docs.rs"));
    assert!(query.contains("site:tokio.rs"));
    assert!(query.contains("OR"));
}

#[test]
fn parser_handles_success_results() {
    let html = r#"
    <a class="result__a" href="https://example.com/redirect?uddg=https%3A%2F%2Ftokio.rs%2Ftokio%2Ftopics%2Fshutdown">Tokio shutdown guide</a>
    <a class="result__snippet">CancellationToken is commonly used for graceful shutdown.</a>
    <a class="result__a" href="https://rust-lang.github.io/async-book/">Async Book</a>
    <a class="result__snippet">Async programming in Rust.</a>
    "#;

    let results = parse_duckduckgo_html(html);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].title, "Tokio shutdown guide");
    assert_eq!(results[0].url, "https://tokio.rs/tokio/topics/shutdown");
    assert!(results[0].snippet.contains("CancellationToken"));
}

#[test]
fn parser_handles_error_results() {
    let html = r#"<html><body><form id="challenge-form">verify</form></body></html>"#;
    assert!(is_bot_challenge(html));
}

#[test]
fn tool_result_includes_all_returned_urls() {
    let output = WebSearchOutput {
        query: "rust async cancellation".into(),
        duration_seconds: 0.1,
        results: vec![
            SearchChunk::Text {
                text: "Useful links found.".into(),
            },
            SearchChunk::Hits {
                tool_use_id: "duckduckgo".into(),
                content: vec![
                    SearchHit {
                        title: "Tokio topic".into(),
                        url: "https://tokio.rs/tokio/topics/shutdown".into(),
                    },
                    SearchHit {
                        title: "Async book".into(),
                        url: "https://rust-lang.github.io/async-book/".into(),
                    },
                ],
            },
        ],
    };

    let text = format_output(&output);
    assert!(text.contains("https://tokio.rs/tokio/topics/shutdown"));
    assert!(text.contains("https://rust-lang.github.io/async-book/"));
}

#[test]
fn domain_filtering_keeps_only_allowed_hosts() {
    let filtered = apply_domain_filters(
        vec![
            RawSearchResult {
                title: "Tokio".into(),
                url: "https://tokio.rs/tokio/topics/shutdown".into(),
                snippet: "A".into(),
            },
            RawSearchResult {
                title: "Example".into(),
                url: "https://example.com/page".into(),
                snippet: "B".into(),
            },
        ],
        &WebSearchInput {
            query: "rust".into(),
            allowed_domains: Some(vec!["tokio.rs".into()]),
            blocked_domains: None,
        },
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].url, "https://tokio.rs/tokio/topics/shutdown");
}

#[test]
fn usage_accounting_marks_search_requests_separately() {
    let details = json!({
        "searchRequests": 1,
        "backend": "duckduckgo-html",
        "cacheHit": false,
    });
    assert_eq!(details["searchRequests"].as_u64(), Some(1));
    assert_eq!(details["backend"].as_str(), Some("duckduckgo-html"));
    assert_eq!(details["cacheHit"].as_bool(), Some(false));
}

#[test]
fn cache_key_normalizes_domain_lists() {
    let a = build_cache_key(&WebSearchInput {
        query: "Rust Async".into(),
        allowed_domains: Some(vec!["tokio.rs".into(), "docs.rs".into()]),
        blocked_domains: None,
    });
    let b = build_cache_key(&WebSearchInput {
        query: "rust async".into(),
        allowed_domains: Some(vec!["docs.rs".into(), "tokio.rs".into()]),
        blocked_domains: None,
    });
    assert_eq!(a, b);
}

#[test]
fn cached_entries_round_trip() {
    let key = "test-cache-key".to_string();
    write_cached_search(
        key.clone(),
        CachedSearchValue {
            query: "rust".into(),
            results: vec![SearchChunk::Text {
                text: "cached summary".into(),
            }],
            fetched_query: "rust".into(),
            hit_count: 1,
        },
    );
    let cached = read_cached_search(&key).expect("cached value");
    assert_eq!(cached.query, "rust");
    assert_eq!(cached.hit_count, 1);
}
