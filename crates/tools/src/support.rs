use bb_core::types::ContentBlock;
use serde_json::Value;
use std::path::PathBuf;

use crate::{ToolContext, ToolResult};

pub(crate) fn emit_progress(ctx: &ToolContext, message: &str) {
    if let Some(on_output) = ctx.on_output.as_deref() {
        on_output(message);
    }
}

pub(crate) fn emit_progress_line(ctx: &ToolContext, message: impl AsRef<str>) {
    let mut line = message.as_ref().to_string();
    line.push('\n');
    emit_progress(ctx, &line);
}

pub(crate) fn build_result(
    content: Vec<ContentBlock>,
    details: Option<Value>,
    is_error: bool,
    artifact_path: Option<PathBuf>,
) -> ToolResult {
    ToolResult {
        content,
        details,
        is_error,
        artifact_path,
    }
}

pub(crate) fn text_result(text: String, details: Option<Value>) -> ToolResult {
    build_result(vec![ContentBlock::Text { text }], details, false, None)
}

pub(crate) fn text_result_with(
    text: String,
    details: Option<Value>,
    is_error: bool,
    artifact_path: Option<PathBuf>,
) -> ToolResult {
    build_result(
        vec![ContentBlock::Text { text }],
        details,
        is_error,
        artifact_path,
    )
}

pub(crate) fn image_result(data: String, mime_type: String) -> ToolResult {
    build_result(
        vec![ContentBlock::Image { data, mime_type }],
        None,
        false,
        None,
    )
}
