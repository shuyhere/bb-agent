use super::super::*;

#[test]
fn edit_tool_result_prefers_diff_when_available() {
    let rendered = format_tool_result_content(
        "edit",
        &[],
        Some(serde_json::json!({
            "path": "/tmp/demo.txt",
            "applied": 1,
            "total": 1,
            "diff": "@@ -1 +1 @@\n-old\n+new"
        })),
        None,
        false,
        false,
    );

    assert!(rendered.contains("applied 1/1 edit(s) to /tmp/demo.txt"));
    assert!(rendered.contains("@@ -1 +1 @@"));
    assert!(rendered.contains("-old"));
    assert!(rendered.contains("+new"));
}

#[test]
fn tool_titles_and_results_shorten_home_paths() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp/test-home".to_string());
    let path = format!("{home}/project/demo.txt");
    let raw_args = serde_json::json!({ "path": path }).to_string();

    let title = format_tool_call_title("read", &raw_args);
    assert!(title.contains("~/project/demo.txt") || title.contains("/project/demo.txt"));

    let rendered = format_tool_result_content(
        "write",
        &[],
        Some(serde_json::json!({
            "path": format!("{home}/project/demo.txt"),
            "bytes": 12
        })),
        None,
        false,
        false,
    );
    assert!(
        rendered.contains("wrote 12 bytes to ~/project/demo.txt")
            || rendered.contains("wrote 12 bytes to /tmp/test-home/project/demo.txt")
    );
}

#[test]
fn tool_titles_include_interactive_context_details() {
    let read = format_tool_call_title(
        "read",
        &serde_json::json!({
            "path": "/tmp/demo.txt",
            "offset": 5,
            "limit": 3
        })
        .to_string(),
    );
    assert_eq!(read, "Read(/tmp/demo.txt:5-7)");

    let ls = format_tool_call_title(
        "ls",
        &serde_json::json!({
            "path": "/tmp",
            "limit": 25
        })
        .to_string(),
    );
    assert_eq!(ls, "LS(/tmp limit=25)");

    let grep = format_tool_call_title(
        "grep",
        &serde_json::json!({
            "pattern": "todo",
            "path": "/tmp/project",
            "glob": "*.rs"
        })
        .to_string(),
    );
    assert_eq!(grep, "Grep(/todo/ /tmp/project *.rs)");

    let find = format_tool_call_title(
        "find",
        &serde_json::json!({
            "pattern": "*.md",
            "path": "/tmp/project"
        })
        .to_string(),
    );
    assert_eq!(find, "Find(*.md /tmp/project)");

    let web_search = format_tool_call_title(
        "web_search",
        &serde_json::json!({
            "query": "Iran United States relations news today"
        })
        .to_string(),
    );
    assert_eq!(
        web_search,
        "WebSearch(\"Iran United States relations news today\")"
    );

    let web_fetch = format_tool_call_title(
        "web_fetch",
        &serde_json::json!({
            "url": "https://example.com/article"
        })
        .to_string(),
    );
    assert_eq!(web_fetch, "WebFetch(https://example.com/article)");

    let browser_fetch = format_tool_call_title(
        "browser_fetch",
        &serde_json::json!({
            "url": "https://example.com/protected"
        })
        .to_string(),
    );
    assert_eq!(browser_fetch, "BrowserFetch(https://example.com/protected)");
}

#[test]
fn bash_title_recovers_from_multiline_non_strict_json_args() {
    let raw = "{\"command\": \"cat > /tmp/cchistory_prompts/full_analysis.py << 'PYEOF'\nimport os\nprint('hi')\nPYEOF\"}";
    let title = format_tool_call_title("bash", raw);
    assert_eq!(
        title,
        "Bash(cat > /tmp/cchistory_prompts/full_analysis.py << 'PYEOF')"
    );
}

#[test]
fn bash_title_skips_common_set_e_prelude_lines() {
    let raw = serde_json::json!({
        "command": "set -e\nset -o pipefail\nprintf hello"
    })
    .to_string();
    let title = format_tool_call_title("bash", &raw);
    assert_eq!(title, "Bash(printf hello)");
}

#[test]
fn bash_title_shows_timeout_when_present() {
    let raw = serde_json::json!({
        "command": "printf hello",
        "timeout": 5
    })
    .to_string();
    let title = format_tool_call_title("bash", &raw);
    assert_eq!(title, "Bash(printf hello timeout=5s)");
}

#[test]
fn bash_title_strips_leading_secret_env_assignments() {
    let raw = serde_json::json!({
        "command": "OPENAI_API_KEY=sk-secret ANTHROPIC_API_KEY=sk-other curl https://example.com"
    })
    .to_string();
    let title = format_tool_call_title("bash", &raw);
    assert_eq!(title, "Bash(curl https://example.com)");
}

#[test]
fn bash_title_redacts_secret_assignments_and_authorization_headers() {
    let raw = serde_json::json!({
        "command": "export OPENAI_API_KEY=sk-secret && curl -H \"Authorization: Bearer sk-top-secret\" https://example.com"
    })
    .to_string();
    let title = format_tool_call_title("bash", &raw);
    assert_eq!(
        title,
        "Bash(export OPENAI_API_KEY=[REDACTED] && curl -H \"Authorization: Bearer [REDACTED]\" https://example.com)"
    );
}

#[test]
fn artifact_paths_shorten_home_prefix() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp/test-home".to_string());
    let rendered = format_tool_result_content(
        "write",
        &[],
        None,
        Some(format!("{home}/project/out.patch")),
        false,
        false,
    );
    assert!(
        rendered.contains("artifact: ~/project/out.patch")
            || rendered.contains("artifact: /tmp/test-home/project/out.patch")
    );
}

#[test]
fn write_and_edit_call_content_use_interactive_style_previews() {
    let write = format_tool_call_content(
        "write",
        &serde_json::json!({
            "path": "/tmp/demo.txt",
            "content": "one\ntwo\nthree\nfour\nfive\nsix"
        })
        .to_string(),
        false,
    );
    assert!(write.contains("one"));
    assert!(write.contains("three"));
    assert!(!write.contains("five"));
    assert!(write.contains(&format!("more lines; {}", crate::ui_hints::TOOL_EXPAND_HINT)));
    assert!(!write.contains("\"content\""));

    let edit = format_tool_call_content(
        "edit",
        &serde_json::json!({
            "path": "/tmp/demo.txt",
            "edits": [
                { "oldText": "alpha", "newText": "beta" },
                { "oldText": "line1\nline2", "newText": "line1\nlineX" }
            ]
        })
        .to_string(),
        false,
    );
    assert!(edit.contains("2 edit block(s)"));
    assert!(edit.contains("1. - alpha"));
    assert!(edit.contains("+ beta"));
    assert!(edit.contains("line1\\nline2"));
    assert!(!edit.contains("\"oldText\""));
}

#[test]
fn tool_result_previews_use_interactive_limits_and_truncation() {
    let bash_lines = (1..=14)
        .map(|i| format!("line\t{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let bash = format_tool_result_content(
        "bash",
        &[ContentBlock::Text {
            text: bash_lines.clone(),
        }],
        None,
        None,
        false,
        false,
    );
    assert!(bash.contains("line   10"));
    assert!(bash.contains("line   14"));
    assert!(bash.contains(&format!("earlier lines; {}", crate::ui_hints::TOOL_EXPAND_HINT)));
    assert!(!bash.contains("line   9"));

    let grep_lines = (1..=16)
        .map(|i| format!("match {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let grep = format_tool_result_content(
        "grep",
        &[ContentBlock::Text {
            text: grep_lines.clone(),
        }],
        None,
        None,
        false,
        false,
    );
    assert!(grep.contains("match 1"));
    assert!(grep.contains("match 3"));
    assert!(grep.contains(&format!("more lines; {}", crate::ui_hints::TOOL_EXPAND_HINT)));
    assert!(!grep.contains("match 4"));

    let expanded = format_tool_result_content(
        "bash",
        &[ContentBlock::Text { text: bash_lines }],
        None,
        None,
        false,
        true,
    );
    assert!(expanded.contains("line   14"));
    assert!(
        !expanded.contains(&crate::ui_hints::more_lines_expand_hint(2))
    );

    let long_lines = (1..=140)
        .map(|i| format!("tail {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let expanded_long = format_tool_result_content(
        "bash",
        &[ContentBlock::Text { text: long_lines }],
        None,
        None,
        false,
        true,
    );
    assert!(expanded_long.contains("… output truncated (21 lines hidden)"));
    assert!(expanded_long.contains("tail 1"));
    assert!(expanded_long.contains("tail 140"));
}

#[test]
fn collapsed_bash_preview_truncates_long_single_line_before_terminal_wrap() {
    let bash = format_tool_result_content(
        "bash",
        &[ContentBlock::Text {
            text: format!("{} tail-marker", "x".repeat(400)),
        }],
        None,
        None,
        false,
        false,
    );
    assert!(bash.contains('…'));
    assert!(!bash.contains("tail-marker"));
}
