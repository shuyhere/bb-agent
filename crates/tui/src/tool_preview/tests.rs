use super::{
    extract_tool_arg_string_relaxed, format_tool_call_content, format_tool_result_content,
};
use bb_core::types::ContentBlock;

#[test]
fn edit_results_keep_old_ui_wider_preview_limit() {
    let text = (1..=200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = format_tool_result_content(
        "edit",
        &[ContentBlock::Text { text }],
        None,
        None,
        false,
        false,
    );
    assert!(rendered.contains("line 40"));
    assert!(rendered.contains("line 200"));
    assert!(!rendered.contains("line 100\nline 101"));
    assert!(rendered.contains("hidden"));
}

#[test]
fn grep_call_body_is_empty_because_title_shows_details() {
    let rendered = format_tool_call_content(
        "grep",
        &serde_json::json!({"pattern":"todo","path":"/tmp","glob":"*.rs"}).to_string(),
        false,
    );
    assert!(rendered.is_empty());
}

#[test]
fn bash_call_body_is_empty_because_header_shows_command_context() {
    let rendered = format_tool_call_content(
        "bash",
        &serde_json::json!({"command":"echo hi\nprintf done","timeout": 5.0}).to_string(),
        false,
    );
    assert!(rendered.is_empty());
}

#[test]
fn bash_invalid_json_args_do_not_dump_raw_command_blob() {
    let raw = "{\"command\": \"cat > /tmp/demo.py << 'PYEOF'\nprint('hi')\nPYEOF\"}";
    let rendered = format_tool_call_content("bash", raw, false);
    assert!(rendered.is_empty());
}

#[test]
fn relaxed_arg_extraction_handles_multiline_bash_command_strings() {
    let raw = "{\"command\": \"cat > /tmp/demo.py << 'PYEOF'\nprint('hi')\nPYEOF\"}";
    let command = extract_tool_arg_string_relaxed(raw, "command").expect("command field");
    assert!(command.starts_with("cat > /tmp/demo.py << 'PYEOF'"));
    assert!(command.contains("print('hi')"));
}

#[test]
fn truncated_preview_mentions_ctrl_o_expand() {
    let text = (1..=14)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = format_tool_result_content(
        "bash",
        &[ContentBlock::Text { text }],
        None,
        None,
        false,
        false,
    );
    assert!(rendered.contains("Ctrl+Shift+O tool expand"));
}

#[test]
fn collapsed_preview_truncates_very_long_single_line() {
    let text = format!("{} tail-marker", "x".repeat(400));
    let rendered = format_tool_result_content(
        "bash",
        &[ContentBlock::Text { text }],
        None,
        None,
        false,
        false,
    );
    assert!(rendered.contains('…'));
    assert!(!rendered.contains("tail-marker"));
}

#[test]
fn web_search_results_render_with_structured_summary_and_links() {
    let rendered = format_tool_result_content(
        "web_search",
        &[ContentBlock::Text {
            text: "Web search results for query: \"Iran United States relations news today\"\nBackend: DuckDuckGo HTML\n\nSummary:\nTop developments here.\n\nLinks:\n- [Example](https://example.com)".to_string(),
        }],
        Some(serde_json::json!({
            "query": "Iran United States relations news today",
            "backend": "duckduckgo-html",
            "cacheHit": true,
            "hitCount": 2,
            "results": [
                {
                    "kind": "Text",
                    "text": "Top developments here."
                },
                {
                    "kind": "Hits",
                    "tool_use_id": "duckduckgo",
                    "content": [
                        { "title": "AP News", "url": "https://apnews.com/story" },
                        { "title": "Reuters", "url": "https://reuters.com/world" }
                    ]
                }
            ]
        })),
        None,
        false,
        false,
    );
    assert!(rendered.contains("query: \"Iran United States relations news today\""));
    assert!(rendered.contains("2 result(s) via duckduckgo-html [cached]"));
    assert!(rendered.contains("summary:"));
    assert!(rendered.contains("links:"));
    assert!(rendered.contains("- AP News — https://apnews.com/story"));
    assert!(rendered.contains("- Reuters — https://reuters.com/world"));
}

#[test]
fn web_fetch_results_render_with_content_and_citation_sections() {
    let rendered = format_tool_result_content(
        "web_fetch",
        &[ContentBlock::Text {
            text: "SECURITY NOTICE\n\nSource: Web Fetch\nURL: https://tokio.rs/tokio/topics/shutdown\n\n---\nFetched body text here.\n\nCitation:\n- [Tokio Shutdown](https://tokio.rs/tokio/topics/shutdown)".to_string(),
        }],
        Some(serde_json::json!({
            "finalUrl": "https://tokio.rs/tokio/topics/shutdown",
            "title": "Tokio Shutdown",
            "contentType": "text/html",
            "extractionSource": "main",
            "truncated": false,
            "citationMarkdown": "- [Tokio Shutdown](https://tokio.rs/tokio/topics/shutdown)"
        })),
        None,
        false,
        false,
    );
    assert!(rendered.contains("title: Tokio Shutdown"));
    assert!(rendered.contains("url: https://tokio.rs/tokio/topics/shutdown"));
    assert!(rendered.contains("text/html | extraction=main"));
    assert!(rendered.contains("content:"));
    assert!(rendered.contains("Fetched body text here."));
    assert!(rendered.contains("citation:"));
    assert!(rendered.contains("- [Tokio Shutdown](https://tokio.rs/tokio/topics/shutdown)"));
}
