use super::*;
use std::path::Path;
use tokio_util::sync::CancellationToken;

fn make_ctx(dir: &Path, execution_policy: crate::ExecutionPolicy) -> ToolContext {
    ToolContext {
        cwd: dir.to_path_buf(),
        artifacts_dir: dir.to_path_buf(),
        execution_policy,
        on_output: None,
        web_search: None,
        execution_mode: crate::ToolExecutionMode::Interactive,
        request_approval: None,
    }
}

#[tokio::test]
async fn write_allows_workspace_paths_in_safety_mode() {
    let dir = tempfile::tempdir().unwrap();
    let tool = WriteTool;

    tool.execute(
        json!({
            "path": "nested/file.txt",
            "content": "hello"
        }),
        &make_ctx(dir.path(), crate::ExecutionPolicy::Safety),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(
        std::fs::read_to_string(dir.path().join("nested/file.txt")).unwrap(),
        "hello"
    );
}

#[tokio::test]
async fn write_rejects_paths_outside_workspace_in_safety_mode() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("blocked.txt");
    let tool = WriteTool;

    let error = tool
        .execute(
            json!({
                "path": outside_file.to_string_lossy(),
                "content": "blocked"
            }),
            &make_ctx(workspace.path(), crate::ExecutionPolicy::Safety),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("restricted to the workspace"));
    assert!(!outside_file.exists());
}

#[tokio::test]
async fn write_allows_paths_outside_workspace_in_yolo_mode() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_file = outside.path().join("allowed.txt");
    let tool = WriteTool;

    tool.execute(
        json!({
            "path": outside_file.to_string_lossy(),
            "content": "allowed"
        }),
        &make_ctx(workspace.path(), crate::ExecutionPolicy::Yolo),
        CancellationToken::new(),
    )
    .await
    .unwrap();

    assert_eq!(std::fs::read_to_string(outside_file).unwrap(), "allowed");
}
