use std::path::{Path, PathBuf};
use uuid::Uuid;

const DEFAULT_THRESHOLD: usize = 100 * 1024; // 100KB

/// Check if content should be offloaded, and if so, save to disk
/// and return a truncated version with a retrieval hint.
pub fn maybe_offload(
    content: &str,
    artifacts_dir: &Path,
    threshold: Option<usize>,
) -> (String, Option<PathBuf>) {
    let threshold = threshold.unwrap_or(DEFAULT_THRESHOLD);

    if content.len() <= threshold {
        return (content.to_string(), None);
    }

    // Ensure directory exists
    if let Err(e) = std::fs::create_dir_all(artifacts_dir) {
        tracing::warn!("Failed to create artifacts dir: {e}");
        return (content.to_string(), None);
    }

    let id = Uuid::new_v4();
    let path = artifacts_dir.join(format!("{id}.txt"));

    if let Err(e) = std::fs::write(&path, content) {
        tracing::warn!("Failed to write artifact: {e}");
        return (content.to_string(), None);
    }

    let truncated = truncate_with_hint(content, threshold, &path);
    (truncated, Some(path))
}

fn safe_char_boundary_at_or_before(s: &str, max_bytes: usize) -> usize {
    let capped = max_bytes.min(s.len());
    if s.is_char_boundary(capped) {
        return capped;
    }
    let mut last = 0usize;
    for (idx, _) in s.char_indices() {
        if idx > capped {
            break;
        }
        last = idx;
    }
    last
}

fn truncate_with_hint(content: &str, max_bytes: usize, path: &Path) -> String {
    // Find last newline before max_bytes to avoid splitting mid-line,
    // but never cut through a UTF-8 character.
    let safe_cut = safe_char_boundary_at_or_before(content, max_bytes);
    let cut = content[..safe_cut].rfind('\n').unwrap_or(safe_cut);

    let preview = &content[..cut];
    let total_lines = content.lines().count();
    let preview_lines = preview.lines().count();

    format!(
        "{preview}\n\n[Truncated. Full output ({total_bytes} bytes, {total_lines} lines) \
         saved to {path}. Use read tool with offset={next_line} to see more.]",
        total_bytes = content.len(),
        path = path.display(),
        next_line = preview_lines + 1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_no_offload_small() {
        let dir = tempfile::tempdir().unwrap();
        let (result, path) = maybe_offload("small content", dir.path(), Some(1000));
        assert_eq!(result, "small content");
        assert!(path.is_none());
    }

    #[test]
    fn test_offload_large() {
        let dir = tempfile::tempdir().unwrap();
        let big = "x".repeat(500);
        let (result, path) = maybe_offload(&big, dir.path(), Some(100));
        assert!(path.is_some());
        assert!(result.contains("[Truncated."));
        // Full content on disk
        let on_disk = fs::read_to_string(path.unwrap()).unwrap();
        assert_eq!(on_disk.len(), 500);
    }

    #[test]
    fn test_offload_large_does_not_split_utf8_characters() {
        let dir = tempfile::tempdir().unwrap();
        let content = format!("{}─tail", "x".repeat(99));
        let (result, path) = maybe_offload(&content, dir.path(), Some(100));
        assert!(path.is_some());
        assert!(result.contains("[Truncated."));
        assert!(!result.contains("�"));
    }
}
