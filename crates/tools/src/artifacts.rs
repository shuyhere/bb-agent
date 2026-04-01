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

fn truncate_with_hint(content: &str, max_bytes: usize, path: &Path) -> String {
    // Find last newline before max_bytes to avoid splitting mid-line
    let cut = content[..max_bytes]
        .rfind('\n')
        .unwrap_or(max_bytes);

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

/// Clean up expired artifacts older than `max_age`.
pub fn cleanup(artifacts_dir: &Path, max_age: std::time::Duration) -> u64 {
    let mut removed = 0u64;
    let entries = match std::fs::read_dir(artifacts_dir) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                if let Ok(age) = now.duration_since(modified) {
                    if age > max_age {
                        if std::fs::remove_file(entry.path()).is_ok() {
                            removed += 1;
                        }
                    }
                }
            }
        }
    }
    removed
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
}
