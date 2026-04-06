use super::*;
use bb_core::types::ContentBlock;
use std::path::Path;
use tokio_util::sync::CancellationToken;

fn make_ctx(dir: &Path) -> ToolContext {
    ToolContext {
        cwd: dir.to_path_buf(),
        artifacts_dir: dir.to_path_buf(),
        on_output: None,
        web_search: None,
    }
}

#[test]
fn safe_char_boundary_never_splits_multibyte_characters() {
    let text = format!("{}─tail", "x".repeat(99));
    let cut = safe_char_boundary_at_or_before(&text, 100);
    assert!(text.is_char_boundary(cut));
    assert_eq!(&text[..cut], &"x".repeat(99));
}

#[tokio::test]
async fn read_simple_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    std::fs::write(&file, "line1\nline2\nline3\n").unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "hello.txt" }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    let text = match &result.content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    assert!(text.contains("line1"));
    assert!(text.contains("line2"));
    assert!(text.contains("line3"));
}

#[tokio::test]
async fn read_with_offset_and_limit() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nums.txt");
    let content: String = (1..=10).map(|i| format!("line{i}\n")).collect();
    std::fs::write(&file, &content).unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "nums.txt", "offset": 3, "limit": 2 }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    assert!(text.contains("line3"));
    assert!(text.contains("line4"));
    assert!(!text.contains("line2"));
    assert!(!text.contains("line5"));
}

#[tokio::test]
async fn read_offset_past_end() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("short.txt");
    std::fs::write(&file, "only\n").unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "short.txt", "offset": 999 }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(result.is_error);
    let text = match &result.content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    assert!(text.contains("past end"));
}

#[tokio::test]
async fn read_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let err = tool
        .execute(
            serde_json::json!({ "path": "nope.txt" }),
            &ctx,
            CancellationToken::new(),
        )
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn read_truncates_large_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("big.txt");
    let content: String = (1..=3000).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&file, &content).unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "big.txt" }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    assert!(text.contains("more lines in file"));
    assert!(!text.contains("line 3000"));
}

#[tokio::test]
async fn read_truncates_by_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("wide.txt");
    let content: String = (1..=500).map(|i| format!("{:0>200}\n", i)).collect();
    std::fs::write(&file, &content).unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "wide.txt" }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    let text = match &result.content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    let full_len: usize = (1..=500)
        .map(|i| format!("{:0>200}", i).len() + 1)
        .sum::<usize>();
    assert!(
        text.len() < full_len,
        "output should be truncated by byte limit"
    );
}

#[tokio::test]
async fn read_utf8_content() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("utf8.txt");
    std::fs::write(&file, "你好世界\nこんにちは\n🎉🎉🎉\n").unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "utf8.txt" }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    let text = match &result.content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    assert!(text.contains("你好世界"));
    assert!(text.contains("こんにちは"));
    assert!(text.contains("🎉🎉🎉"));
}

#[tokio::test]
async fn read_image_returns_base64() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.png");
    let png_bytes: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE,
    ];
    std::fs::write(&file, png_bytes).unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "test.png" }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    match &result.content[0] {
        ContentBlock::Image { data, mime_type } => {
            assert_eq!(mime_type, "image/png");
            assert!(!data.is_empty());
        }
        _ => panic!("expected image content block"),
    }
}

#[tokio::test]
async fn read_absolute_path() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("abs.txt");
    std::fs::write(&file, "absolute content\n").unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": file.to_str().unwrap() }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    let text = match &result.content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    assert!(text.contains("absolute content"));
}

#[tokio::test]
async fn read_strips_at_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("at.txt");
    std::fs::write(&file, "at content\n").unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "@at.txt" }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    let text = match &result.content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    assert!(text.contains("at content"));
}

#[tokio::test]
async fn read_returns_correct_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("meta.txt");
    std::fs::write(&file, "a\nb\nc\nd\ne\n").unwrap();

    let tool = ReadTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            serde_json::json!({ "path": "meta.txt", "offset": 2, "limit": 2 }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    let details = result.details.unwrap();
    assert_eq!(details["totalLines"], 5);
    assert_eq!(details["startLine"], 2);
    assert_eq!(details["endLine"], 3);
}
