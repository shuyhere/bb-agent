use std::path::Path;

use super::*;

const CLIPBOARD_IMAGE_PREFIX: &str = "bb-clipboard-";

impl TuiState {
    /// Called when an image file is attached (via clipboard read or drag-and-drop).
    /// Stores the path and relies on the input block chips for visual feedback.
    pub fn on_image_attached(&mut self, path: String, _size_bytes: u64) {
        self.pending_image_paths.push(path);
        self.dirty = true;
    }

    /// Take pending image paths (called by controller on submit).
    pub fn take_pending_image_paths(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_image_paths)
    }

    pub fn on_paste(&mut self, text: &str) {
        if self.mode == TuiMode::Normal && self.suppress_next_paste_payload {
            self.suppress_next_paste_payload = false;
            return;
        }

        if self.mode == TuiMode::Normal
            && let Some((path, size_bytes)) = try_read_clipboard_image()
        {
            self.on_image_attached(path, size_bytes);
            self.suppress_next_paste_payload = true;
            return;
        }

        if self.mode == TuiMode::Normal
            && let Some(handled) = self.handle_pasted_paths(text)
        {
            self.status_line = handled;
            self.dirty = true;
            return;
        }

        match self.mode {
            TuiMode::Normal => self.handle_paste(text),
            TuiMode::Transcript => {
                self.status_line =
                    "paste is ignored while transcript navigation is active".to_string();
                self.dirty = true;
            }
        }
    }

    fn handle_pasted_paths(&mut self, text: &str) -> Option<String> {
        let paths = parse_pasted_local_paths(text);
        if paths.is_empty() {
            return None;
        }

        let mut attached_images = 0usize;
        let mut inserted_files = Vec::new();
        for path in paths {
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if is_image_path(&path) {
                self.on_image_attached(path.clone(), meta.len());
                attached_images += 1;
            } else {
                inserted_files.push(format_at_file_reference(&path));
            }
        }

        if !inserted_files.is_empty() {
            if should_insert_leading_space(&self.input, self.cursor) {
                self.insert_str(" ");
            }
            let refs = inserted_files.join(" ");
            self.insert_str(&refs);
            if should_insert_trailing_space(&self.input, self.cursor) {
                self.insert_str(" ");
            }
        }

        match (attached_images, inserted_files.len()) {
            (0, 0) => None,
            (images, 0) => Some(format!(
                "Attached {images} image(s). Type your prompt and press Enter."
            )),
            (0, files) => Some(format!(
                "Inserted {files} file reference(s) into the prompt."
            )),
            (images, files) => Some(format!(
                "Attached {images} image(s) and inserted {files} file reference(s)."
            )),
        }
    }

    /// Handle pasted text. Large pastes (>10 lines or >1000 chars) are collapsed
    /// into a `[paste #N +XX lines]` marker to keep the editor readable.
    /// The full content is stored and expanded on submit.
    fn handle_paste(&mut self, text: &str) {
        let sanitized = sanitize_pasted_text(text);
        if sanitized.is_empty() {
            self.status_line = "empty paste ignored".to_string();
            self.dirty = true;
            return;
        }

        let line_count = sanitized.lines().count();
        let char_count = sanitized.len();

        if line_count > 10 || char_count > 1000 {
            self.paste_counter += 1;
            let id = self.paste_counter;
            let marker = if line_count > 10 {
                format!("[paste #{id} +{line_count} lines]")
            } else {
                format!("[paste #{id} {char_count} chars]")
            };
            self.paste_storage.insert(id, sanitized);
            self.insert_str(&marker);
        } else {
            self.insert_str(&sanitized);
        }
    }
}

fn sanitize_pasted_text(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    normalized
        .chars()
        .filter(|ch| *ch == '\n' || *ch == '\t' || !ch.is_control())
        .collect()
}

/// Try to read an image from the system clipboard using available tools.
/// Returns `(temp_file_path, file_size)` on success.
pub fn try_read_clipboard_image() -> Option<(String, u64)> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let tmp_path = std::env::temp_dir().join(format!(
        "{CLIPBOARD_IMAGE_PREFIX}{}-{timestamp}.png",
        std::process::id()
    ));
    let tmp_path_str = tmp_path.to_string_lossy().to_string();

    if try_clipboard_command(
        "wl-paste",
        &["--type", "image/png", "--no-newline"],
        &tmp_path_str,
    ) && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path_str, meta.len()));
    }

    if try_clipboard_command(
        "xclip",
        &["-selection", "clipboard", "-t", "image/png", "-o"],
        &tmp_path_str,
    ) && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path_str, meta.len()));
    }

    if command_status_quiet(std::process::Command::new("pngpaste").arg(&tmp_path))
        && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path_str, meta.len()));
    }

    if try_macos_applescript_save_clipboard_image(&tmp_path_str)
        && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path_str, meta.len()));
    }

    if try_macos_save_clipboard_image(&tmp_path_str)
        && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path_str, meta.len()));
    }

    if try_powershell_save_clipboard_image(&tmp_path_str)
        && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path_str, meta.len()));
    }

    if command_status_quiet(std::process::Command::new("grab-screenshot").arg(&tmp_path))
        && let Ok(meta) = std::fs::metadata(&tmp_path)
        && meta.len() > 0
    {
        return Some((tmp_path_str, meta.len()));
    }

    let _ = std::fs::remove_file(&tmp_path);
    None
}

pub(crate) fn is_managed_clipboard_temp_image(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    path.parent() == Some(std::env::temp_dir().as_path())
        && file_name.starts_with(CLIPBOARD_IMAGE_PREFIX)
        && file_name.ends_with(".png")
}

pub(crate) fn cleanup_managed_clipboard_temp_image(path: &Path) {
    if is_managed_clipboard_temp_image(path) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn try_read_clipboard_text() -> Option<String> {
    read_clipboard_text_command("pbpaste", &[])
        .or_else(|| read_clipboard_text_command("wl-paste", &["--no-newline"]))
        .or_else(|| read_clipboard_text_command("xclip", &["-selection", "clipboard", "-o"]))
        .or_else(|| {
            read_clipboard_text_command(
                "powershell",
                &["-NoProfile", "-Command", "Get-Clipboard -Raw"],
            )
        })
        .or_else(|| {
            read_clipboard_text_command("pwsh", &["-NoProfile", "-Command", "Get-Clipboard -Raw"])
        })
        .map(|text| text.replace("\r\n", "\n"))
        .filter(|text| !text.trim().is_empty())
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

fn read_clipboard_text_command(cmd: &str, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn command_status_quiet(command: &mut std::process::Command) -> bool {
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn try_macos_applescript_save_clipboard_image(output_path: &str) -> bool {
    let escaped = output_path.replace('"', "\\\"");
    let script = format!(
        "set outPath to POSIX file \"{escaped}\"\ntry\n    set pngData to the clipboard as «class PNGf»\non error\n    return false\nend try\nset fileRef to open for access outPath with write permission\ntry\n    set eof fileRef to 0\n    write pngData to fileRef\n    close access fileRef\n    return true\non error\n    try\n        close access fileRef\n    end try\n    return false\nend try"
    );

    command_status_quiet(std::process::Command::new("osascript").args(["-e", &script]))
}

fn try_macos_save_clipboard_image(output_path: &str) -> bool {
    let escaped = output_path.replace('"', "\\\"");
    let script = format!(
        "ObjC.import('AppKit');\nObjC.import('Foundation');\nconst path = \"{escaped}\";\nconst fm = $.NSFileManager.defaultManager;\nconst pb = $.NSPasteboard.generalPasteboard;\nfunction cleanup() {{ fm.removeItemAtPathError($(path), null); }}\nfunction writePngData(data) {{ return data && data.writeToFileAtomically($(path), true); }}\nfunction pngFromTiffData(tiff) {{\n  if (!tiff) return null;\n  const rep = $.NSBitmapImageRep.imageRepWithData(tiff);\n  if (!rep) return null;\n  return rep.representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $.NSDictionary.dictionary);\n}}\nlet data = pb.dataForType('public.png');\nif (!data) data = pb.dataForType('PNGf');\nif (data && writePngData(data)) {{ $.exit(0); }}\nlet tiff = pb.dataForType('public.tiff');\nif (!tiff) tiff = pb.dataForType('NSTIFFPboardType');\nlet png = pngFromTiffData(tiff);\nif (png && writePngData(png)) {{ $.exit(0); }}\nconst classes = $.NSArray.arrayWithObject($.NSImage);\nconst images = pb.readObjectsForClassesOptions(classes, $.NSDictionary.dictionary);\nif (images && images.count > 0) {{\n  const image = images.objectAtIndex(0);\n  const png2 = pngFromTiffData(image.TIFFRepresentation);\n  if (png2 && writePngData(png2)) {{ $.exit(0); }}\n}}\ncleanup();\n$.exit(1);"
    );

    command_status_quiet(std::process::Command::new("osascript").args([
        "-l",
        "JavaScript",
        "-e",
        &script,
    ]))
}

fn try_powershell_save_clipboard_image(output_path: &str) -> bool {
    let escaped = output_path.replace('\'', "''");
    let script = format!(
        "$img = Get-Clipboard -Format Image; if ($null -eq $img) {{ exit 1 }}; Add-Type -AssemblyName System.Drawing; $img.Save('{escaped}', [System.Drawing.Imaging.ImageFormat]::Png)"
    );

    for shell in ["powershell", "pwsh"] {
        if command_status_quiet(std::process::Command::new(shell).args([
            "-NoProfile",
            "-Command",
            &script,
        ])) {
            return true;
        }
    }

    false
}

fn should_insert_leading_space(input: &str, cursor: usize) -> bool {
    input[..cursor]
        .chars()
        .next_back()
        .map(|ch| !ch.is_whitespace())
        .unwrap_or(false)
}

fn should_insert_trailing_space(input: &str, cursor: usize) -> bool {
    input[cursor..]
        .chars()
        .next()
        .map(|ch| !ch.is_whitespace())
        .unwrap_or(false)
}

fn format_at_file_reference(path: &str) -> String {
    if path.chars().any(char::is_whitespace) {
        let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
        format!("@\"{escaped}\"")
    } else {
        format!("@{path}")
    }
}

fn parse_pasted_local_paths(text: &str) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if let Some(path) = normalize_local_path_candidate(trimmed)
        && std::fs::metadata(&path).is_ok()
    {
        return vec![path];
    }

    let mut out = Vec::new();
    for line in trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let Some(path) = normalize_local_path_candidate(line) else {
            return Vec::new();
        };
        if std::fs::metadata(&path).is_err() {
            return Vec::new();
        }
        if !out.iter().any(|existing| existing == &path) {
            out.push(path);
        }
    }
    out
}

fn normalize_local_path_candidate(input: &str) -> Option<String> {
    let mut candidate = input.trim().trim_matches(|ch| matches!(ch, '\n' | '\r'));
    if candidate.is_empty() {
        return None;
    }

    if (candidate.starts_with('"') && candidate.ends_with('"'))
        || (candidate.starts_with('\'') && candidate.ends_with('\''))
    {
        candidate = &candidate[1..candidate.len().saturating_sub(1)];
    }

    let path = if candidate.starts_with("file://") {
        let url = url::Url::parse(candidate).ok()?;
        url.to_file_path().ok()?.to_string_lossy().to_string()
    } else if candidate == "~" {
        std::env::var("HOME").ok()?
    } else if let Some(rest) = candidate.strip_prefix("~/") {
        format!("{}/{}", std::env::var("HOME").ok()?, rest)
    } else {
        candidate.to_string()
    };

    if std::fs::metadata(&path).is_ok() {
        return Some(path);
    }

    let unescaped = path
        .replace("\\ ", " ")
        .replace("\\(", "(")
        .replace("\\)", ")")
        .replace("\\[", "[")
        .replace("\\]", "]");
    if std::fs::metadata(&unescaped).is_ok() {
        return Some(unescaped);
    }

    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_quoted_file_uri_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let file = temp.path().join("hello world.txt");
        std::fs::write(&file, "hi").expect("write file");
        let uri = format!("\"{}\"", url::Url::from_file_path(&file).expect("file url"));

        let normalized = normalize_local_path_candidate(&uri).expect("normalized path");
        assert_eq!(std::path::Path::new(&normalized), file.as_path());
    }

    #[test]
    fn parses_multiple_newline_separated_paths() {
        let temp = tempfile::tempdir().expect("temp dir");
        let a = temp.path().join("a.txt");
        let b = temp.path().join("b.png");
        std::fs::write(&a, "a").expect("write a");
        std::fs::write(&b, "b").expect("write b");

        let pasted = format!("{}\n{}", a.display(), b.display());
        let parsed = parse_pasted_local_paths(&pasted);

        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn formats_spaced_paths_as_quoted_at_refs() {
        assert_eq!(
            format_at_file_reference("/tmp/hello world.txt"),
            "@\"/tmp/hello world.txt\""
        );
    }

    #[test]
    fn sanitizes_carriage_returns_and_control_chars() {
        let sanitized = sanitize_pasted_text("hello\r\nworld\u{1b}[31m");
        assert_eq!(sanitized, "hello\nworld[31m");
    }

    #[test]
    fn recognizes_managed_clipboard_temp_images() {
        let path = std::env::temp_dir().join("bb-clipboard-123-456.png");
        assert!(is_managed_clipboard_temp_image(&path));

        let other = std::env::temp_dir().join("not-bb-clipboard.png");
        assert!(!is_managed_clipboard_temp_image(&other));
    }

    #[test]
    fn cleanup_removes_managed_clipboard_temp_images() {
        let path = std::env::temp_dir().join(format!(
            "bb-clipboard-test-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time after epoch")
                .as_nanos()
        ));
        std::fs::write(&path, b"png-bytes").expect("write temp image");

        cleanup_managed_clipboard_temp_image(&path);

        assert!(!path.exists());
    }
}
