use super::*;
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

fn read_file(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap()
}

#[tokio::test]
async fn edit_single_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.txt");
    std::fs::write(&file, "hello world\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "test.txt",
                "edits": [{ "oldText": "hello", "newText": "goodbye" }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert_eq!(read_file(dir.path(), "test.txt"), "goodbye world\n");
    let details = result.details.unwrap();
    assert_eq!(details["applied"], 1);
    assert_eq!(details["total"], 1);
}

#[tokio::test]
async fn edit_multiple_replacements() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("multi.txt");
    std::fs::write(&file, "aaa\nbbb\nccc\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "multi.txt",
                "edits": [
                    { "oldText": "aaa", "newText": "AAA" },
                    { "oldText": "ccc", "newText": "CCC" }
                ]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert_eq!(read_file(dir.path(), "multi.txt"), "AAA\nbbb\nCCC\n");
    let details = result.details.unwrap();
    assert_eq!(details["applied"], 2);
}

#[tokio::test]
async fn edit_old_text_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nf.txt");
    std::fs::write(&file, "hello\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "nf.txt",
                "edits": [{ "oldText": "missing", "newText": "replaced" }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(result.is_error);
    let details = result.details.unwrap();
    assert_eq!(details["applied"], 0);
    let errors = details["errors"].as_array().unwrap();
    assert!(errors[0].as_str().unwrap().contains("not found"));
    assert_eq!(read_file(dir.path(), "nf.txt"), "hello\n");
}

#[tokio::test]
async fn edit_duplicate_match_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dup.txt");
    std::fs::write(&file, "foo bar foo\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "dup.txt",
                "edits": [{ "oldText": "foo", "newText": "baz" }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(result.is_error);
    let details = result.details.unwrap();
    assert_eq!(details["applied"], 0);
    let errors = details["errors"].as_array().unwrap();
    assert!(errors[0].as_str().unwrap().contains("matches 2 locations"));
    assert_eq!(read_file(dir.path(), "dup.txt"), "foo bar foo\n");
}

#[tokio::test]
async fn edit_empty_old_text() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("empty.txt");
    std::fs::write(&file, "content\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "empty.txt",
                "edits": [{ "oldText": "", "newText": "stuff" }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(result.is_error);
    let details = result.details.unwrap();
    assert_eq!(details["applied"], 0);
    assert_eq!(read_file(dir.path(), "empty.txt"), "content\n");
}

#[tokio::test]
async fn edit_empty_edits_array() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("e.txt");
    std::fs::write(&file, "hi\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let err = tool
        .execute(
            json!({ "path": "e.txt", "edits": [] }),
            &ctx,
            CancellationToken::new(),
        )
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn edit_file_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let err = tool
        .execute(
            json!({
                "path": "nonexistent.txt",
                "edits": [{ "oldText": "a", "newText": "b" }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn edit_partial_success() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("partial.txt");
    std::fs::write(&file, "alpha beta gamma\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "partial.txt",
                "edits": [
                    { "oldText": "alpha", "newText": "ALPHA" },
                    { "oldText": "missing", "newText": "nope" }
                ]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(result.is_error);
    let details = result.details.unwrap();
    assert_eq!(details["applied"], 1);
    assert_eq!(details["total"], 2);
    assert_eq!(read_file(dir.path(), "partial.txt"), "ALPHA beta gamma\n");
}

#[tokio::test]
async fn edit_matches_against_original_file_not_mutated_content() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("original.txt");
    std::fs::write(&file, "abcde\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "original.txt",
                "edits": [
                    { "oldText": "bc", "newText": "X" },
                    { "oldText": "Xde", "newText": "Y" }
                ]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(result.is_error);
    let details = result.details.unwrap();
    assert_eq!(details["applied"], 1);
    let errors = details["errors"].as_array().unwrap();
    assert!(errors.iter().any(|error| {
        error
            .as_str()
            .unwrap()
            .contains("Edit 1: oldText not found in file")
    }));
    assert_eq!(read_file(dir.path(), "original.txt"), "aXde\n");
}

#[tokio::test]
async fn edit_rejects_overlapping_ranges_in_original_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("overlap.txt");
    std::fs::write(&file, "abcde\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "overlap.txt",
                "edits": [
                    { "oldText": "bc", "newText": "BC" },
                    { "oldText": "cd", "newText": "CD" }
                ]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(result.is_error);
    let details = result.details.unwrap();
    assert_eq!(details["applied"], 0);
    let errors = details["errors"].as_array().unwrap();
    assert!(errors.iter().any(|error| {
        error
            .as_str()
            .unwrap()
            .contains("overlap in the original file")
    }));
    assert_eq!(read_file(dir.path(), "overlap.txt"), "abcde\n");
}

#[tokio::test]
async fn edit_multiline_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("ml.txt");
    std::fs::write(&file, "fn main() {\n    println!(\"old\");\n}\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "ml.txt",
                "edits": [{
                    "oldText": "fn main() {\n    println!(\"old\");\n}",
                    "newText": "fn main() {\n    println!(\"new\");\n    println!(\"extra\");\n}"
                }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    let content = read_file(dir.path(), "ml.txt");
    assert!(content.contains("new"));
    assert!(content.contains("extra"));
    assert!(!content.contains("old"));
}

#[tokio::test]
async fn edit_generates_diff() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("diff.txt");
    std::fs::write(&file, "line1\nline2\nline3\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "diff.txt",
                "edits": [{ "oldText": "line2", "newText": "LINE2" }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    let details = result.details.unwrap();
    let diff = details["diff"].as_str().unwrap();
    assert!(diff.contains("line2"));
    assert!(diff.contains("LINE2"));
}

#[tokio::test]
async fn edit_utf8_content() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("utf8.txt");
    std::fs::write(&file, "你好世界\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "utf8.txt",
                "edits": [{ "oldText": "你好", "newText": "再见" }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert_eq!(read_file(dir.path(), "utf8.txt"), "再见世界\n");
}

#[tokio::test]
async fn edit_strips_at_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("at.txt");
    std::fs::write(&file, "old text\n").unwrap();

    let tool = EditTool;
    let ctx = make_ctx(dir.path());
    let result = tool
        .execute(
            json!({
                "path": "@at.txt",
                "edits": [{ "oldText": "old", "newText": "new" }]
            }),
            &ctx,
            CancellationToken::new(),
        )
        .await
        .unwrap();

    assert!(!result.is_error);
    assert_eq!(read_file(dir.path(), "at.txt"), "new text\n");
}
