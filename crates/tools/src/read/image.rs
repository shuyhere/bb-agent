use bb_core::error::{BbError, BbResult};
use std::path::Path;

use crate::{ToolResult, support::image_result};

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

pub(super) fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub(super) async fn read_image(path: &Path) -> BbResult<ToolResult> {
    let data = tokio::fs::read(path)
        .await
        .map_err(|e| BbError::Tool(format!("Failed to read image: {e}")))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();

    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    };

    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);

    Ok(image_result(encoded, mime.to_string()))
}
