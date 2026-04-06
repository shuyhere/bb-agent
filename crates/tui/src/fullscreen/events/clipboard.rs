use super::*;

impl FullscreenState {
    /// Called when an image file is attached (via Ctrl+V clipboard read or drag-and-drop).
    /// Stores the path and updates the status line to show the attachment.
    pub fn on_image_attached(&mut self, path: String, size_bytes: u64) {
        let display = if let Some(name) = std::path::Path::new(&path).file_name() {
            name.to_string_lossy().to_string()
        } else {
            path.clone()
        };
        let size_kb = size_bytes / 1024;
        self.pending_image_paths.push(path);
        let count = self.pending_image_paths.len();
        self.status_line =
            format!("📎 {display} ({size_kb}KB) attached — {count} image(s) pending");
        self.dirty = true;
    }

    /// Take pending image paths (called by controller on submit).
    pub fn take_pending_image_paths(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_image_paths)
    }

    pub fn on_paste(&mut self, text: &str) {
        if self.mode == FullscreenMode::Normal {
            let trimmed = text.trim();
            if is_image_path(trimmed)
                && let Ok(meta) = std::fs::metadata(trimmed)
            {
                self.on_image_attached(trimmed.to_string(), meta.len());
                return;
            }
        }

        match self.mode {
            FullscreenMode::Normal => self.handle_paste(text),
            FullscreenMode::Transcript => {
                self.status_line =
                    "paste is ignored while transcript navigation is active".to_string();
                self.dirty = true;
            }
        }
    }

    /// Handle pasted text. Large pastes (>10 lines or >1000 chars) are collapsed
    /// into a `[paste #N +XX lines]` marker to keep the editor readable.
    /// The full content is stored and expanded on submit.
    fn handle_paste(&mut self, text: &str) {
        let line_count = text.lines().count();
        let char_count = text.len();

        if line_count > 10 || char_count > 1000 {
            self.paste_counter += 1;
            let id = self.paste_counter;
            let marker = if line_count > 10 {
                format!("[paste #{id} +{line_count} lines]")
            } else {
                format!("[paste #{id} {char_count} chars]")
            };
            self.paste_storage.insert(id, text.to_string());
            self.insert_str(&marker);
        } else {
            self.insert_str(text);
        }
    }
}

/// Try to read an image from the system clipboard using available tools.
/// Returns `(temp_file_path, file_size)` on success.
pub(super) fn try_read_clipboard_image() -> Option<(String, u64)> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let tmp_path = format!("/tmp/bb-clipboard-{timestamp}.png");

    if try_clipboard_command(
        "wl-paste",
        &["--type", "image/png", "--no-newline"],
        &tmp_path,
    ) && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path, meta.len()));
    }

    if try_clipboard_command(
        "xclip",
        &["-selection", "clipboard", "-t", "image/png", "-o"],
        &tmp_path,
    ) && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path, meta.len()));
    }

    if let Ok(output) = std::process::Command::new("grab-screenshot")
        .arg(&tmp_path)
        .output()
        && output.status.success()
        && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path, meta.len()));
    }

    let _ = std::fs::remove_file(&tmp_path);
    None
}

/// Run a clipboard command and redirect stdout to a file.
fn try_clipboard_command(cmd: &str, args: &[&str], output_path: &str) -> bool {
    let Ok(output) = std::process::Command::new(cmd).args(args).output() else {
        return false;
    };
    if !output.status.success() || output.stdout.is_empty() {
        return false;
    }
    std::fs::write(output_path, &output.stdout).is_ok()
}

/// Check if a string looks like a path to a supported image file.
pub(super) fn is_image_path(s: &str) -> bool {
    if s.contains('\n') || s.contains('\r') || s.is_empty() {
        return false;
    }
    let lower = s.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
}
