use std::path::{Path, PathBuf};

const FULL_FILE_MAX_BYTES: usize = 12_000;
const FULL_FILE_MAX_LINES: usize = 200;
const DIRECTORY_TREE_MAX_DEPTH: usize = 4;
const DIRECTORY_TREE_MAX_ENTRIES: usize = 120;
const LARGE_FILE_OUTLINE_MAX_ITEMS: usize = 60;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ExpandedInputFiles {
    pub text: String,
    pub expanded_paths: Vec<String>,
    pub warnings: Vec<String>,
}

pub(crate) fn expand_at_file_references(text: &str, cwd: &Path) -> ExpandedInputFiles {
    let mut out = String::new();
    let mut warnings = Vec::new();
    let mut expanded_paths = Vec::new();
    let mut cursor = 0usize;

    while cursor < text.len() {
        let Some(ch) = text[cursor..].chars().next() else {
            break;
        };

        if ch == '@'
            && is_at_reference_boundary(text, cursor)
            && let Some((end, raw_path)) = parse_at_reference(text, cursor)
        {
            let resolved = resolve_reference_path(&raw_path, cwd);
            let display_path = display_path_for_prompt(&resolved, cwd);

            match build_reference_expansion(&resolved, &display_path) {
                Ok(Some(expanded)) => {
                    out.push_str(&expanded);
                    expanded_paths.push(display_path);
                    cursor = end;
                    continue;
                }
                Ok(None) => {}
                Err(message) => warnings.push(message),
            }
        }

        out.push(ch);
        cursor += ch.len_utf8();
    }

    ExpandedInputFiles {
        text: out,
        expanded_paths,
        warnings,
    }
}

fn build_reference_expansion(path: &Path, display_path: &str) -> Result<Option<String>, String> {
    if !path.exists() {
        return Ok(None);
    }

    if path.is_dir() {
        return Ok(Some(render_directory_tree(path, display_path)));
    }

    let bytes = std::fs::read(path).map_err(|_| format!("Could not read file {display_path}"))?;
    let text = match String::from_utf8(bytes.clone()) {
        Ok(text) => text,
        Err(_) => return Ok(Some(render_non_utf8_file_note(display_path, bytes.len()))),
    };
    let line_count = text.lines().count();

    if bytes.len() <= FULL_FILE_MAX_BYTES && line_count <= FULL_FILE_MAX_LINES {
        return Ok(Some(format!(
            "Contents of {display_path}:\n```\n{text}\n```"
        )));
    }

    Ok(Some(render_large_file_outline(
        path,
        display_path,
        &text,
        bytes.len(),
        line_count,
    )))
}

fn render_non_utf8_file_note(display_path: &str, byte_len: usize) -> String {
    format!(
        "File metadata for {display_path}:\n- non-UTF-8 or binary file\n- size: {} bytes\n- contents were not inlined automatically",
        byte_len
    )
}

fn render_directory_tree(path: &Path, display_path: &str) -> String {
    let mut entries = Vec::new();
    let mut total = 0usize;
    collect_directory_tree(
        path,
        0,
        &mut total,
        &mut entries,
        DIRECTORY_TREE_MAX_DEPTH,
        DIRECTORY_TREE_MAX_ENTRIES,
    );

    let mut out = format!("Directory tree for {display_path}:\n```text\n{display_path}/\n");
    for line in entries {
        out.push_str(&line);
        out.push('\n');
    }
    if total > DIRECTORY_TREE_MAX_ENTRIES {
        out.push_str(&format!(
            "... (showing first {DIRECTORY_TREE_MAX_ENTRIES} entries)\n"
        ));
    }
    out.push_str("```");
    out
}

fn collect_directory_tree(
    path: &Path,
    depth: usize,
    total: &mut usize,
    out: &mut Vec<String>,
    max_depth: usize,
    max_entries: usize,
) {
    if depth >= max_depth || *total >= max_entries {
        return;
    }

    let Ok(read_dir) = std::fs::read_dir(path) else {
        return;
    };

    let mut entries = read_dir
        .filter_map(|entry| entry.ok())
        .filter(|entry| !should_skip_tree_entry(&entry.path()))
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| {
        let a_is_dir = a.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        let b_is_dir = b.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        match (a_is_dir, b_is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    for entry in entries {
        if *total >= max_entries {
            break;
        }
        *total += 1;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        let indent = "  ".repeat(depth + 1);
        out.push(format!("{indent}{}{}", name, if is_dir { "/" } else { "" }));
        if is_dir {
            collect_directory_tree(&entry.path(), depth + 1, total, out, max_depth, max_entries);
        }
    }
}

fn should_skip_tree_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | "__pycache__" | ".venv"
            )
        })
        .unwrap_or(false)
}

fn render_large_file_outline(
    path: &Path,
    display_path: &str,
    text: &str,
    byte_len: usize,
    line_count: usize,
) -> String {
    let outline = extract_outline_items(path, text);
    let mut out = format!(
        "File outline for {display_path}:\n- size: {byte_len} bytes\n- lines: {line_count}\n- file is large, so BB inlined structure first instead of full contents\n"
    );

    if outline.is_empty() {
        out.push_str("- no strong top-level symbols detected automatically\n");
    } else {
        out.push_str("- top-level items:\n");
        for item in outline.into_iter().take(LARGE_FILE_OUTLINE_MAX_ITEMS) {
            out.push_str(&format!("  - {item}\n"));
        }
    }

    out
}

fn extract_outline_items(path: &Path, text: &str) -> Vec<String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut items = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            if extension == "md" && trimmed.starts_with('#') {
                items.push(format!("heading {trimmed}"));
            }
            continue;
        }

        let normalized = trimmed
            .strip_prefix("pub ")
            .or_else(|| trimmed.strip_prefix("export "))
            .unwrap_or(trimmed);

        if let Some(name) = normalized.strip_prefix("fn ") {
            items.push(format!("fn {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("async fn ") {
            items.push(format!("async fn {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("struct ") {
            items.push(format!("struct {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("enum ") {
            items.push(format!("enum {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("trait ") {
            items.push(format!("trait {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("impl ") {
            items.push(format!("impl {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("mod ") {
            items.push(format!("mod {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("type ") {
            items.push(format!("type {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("const ") {
            items.push(format!("const {}", take_symbol_name(name)));
        } else if let Some(name) = normalized.strip_prefix("let ") {
            if items.len() < 5 {
                items.push(format!("binding {}", take_symbol_name(name)));
            }
        } else if extension == "py" {
            if let Some(name) = trimmed.strip_prefix("def ") {
                items.push(format!("def {}", take_symbol_name(name)));
            } else if let Some(name) = trimmed.strip_prefix("async def ") {
                items.push(format!("async def {}", take_symbol_name(name)));
            } else if let Some(name) = trimmed.strip_prefix("class ") {
                items.push(format!("class {}", take_symbol_name(name)));
            }
        } else if matches!(extension.as_str(), "ts" | "tsx" | "js" | "jsx") {
            if let Some(name) = trimmed.strip_prefix("function ") {
                items.push(format!("function {}", take_symbol_name(name)));
            } else if let Some(name) = trimmed.strip_prefix("class ") {
                items.push(format!("class {}", take_symbol_name(name)));
            } else if let Some(name) = trimmed.strip_prefix("interface ") {
                items.push(format!("interface {}", take_symbol_name(name)));
            }
        } else if extension == "go" {
            if let Some(name) = trimmed.strip_prefix("func ") {
                items.push(format!("func {}", take_symbol_name(name)));
            } else if let Some(name) = trimmed.strip_prefix("type ") {
                items.push(format!("type {}", take_symbol_name(name)));
            }
        }

        if items.len() >= LARGE_FILE_OUTLINE_MAX_ITEMS {
            break;
        }
    }

    dedupe_preserve_order(items)
}

fn take_symbol_name(rest: &str) -> String {
    rest.chars()
        .take_while(|ch| !matches!(ch, '(' | '{' | ':' | '<' | '=' | ' ' | '\t'))
        .collect::<String>()
}

fn dedupe_preserve_order(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if !item.is_empty() && !out.iter().any(|existing| existing == &item) {
            out.push(item);
        }
    }
    out
}

fn is_at_reference_boundary(text: &str, at_pos: usize) -> bool {
    if at_pos == 0 {
        return true;
    }
    text[..at_pos]
        .chars()
        .next_back()
        .map(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{'))
        .unwrap_or(true)
}

fn parse_at_reference(text: &str, at_pos: usize) -> Option<(usize, String)> {
    let rest = text.get(at_pos + 1..)?;
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;

    if first == '"' || first == '\'' {
        let quote = first;
        let mut value = String::new();
        let mut escaped = false;
        for (idx, ch) in rest[first.len_utf8()..].char_indices() {
            if escaped {
                value.push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == quote {
                let end = at_pos + 1 + first.len_utf8() + idx + ch.len_utf8();
                return Some((end, value));
            }
            value.push(ch);
        }
        return None;
    }

    let mut end = at_pos + 1;
    for (idx, ch) in rest.char_indices() {
        if ch.is_whitespace() {
            break;
        }
        end = at_pos + 1 + idx + ch.len_utf8();
    }

    if end <= at_pos + 1 {
        None
    } else {
        Some((end, text[at_pos + 1..end].to_string()))
    }
}

fn resolve_reference_path(raw_path: &str, cwd: &Path) -> PathBuf {
    let trimmed = raw_path.trim();
    if let Some(expanded) = expand_home(trimmed) {
        return expanded;
    }
    if trimmed.starts_with("file://")
        && let Ok(url) = url::Url::parse(trimmed)
        && let Ok(path) = url.to_file_path()
    {
        return path;
    }

    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

fn expand_home(path: &str) -> Option<PathBuf> {
    if path == "~" {
        return std::env::var_os("HOME").map(PathBuf::from);
    }
    let suffix = path.strip_prefix("~/")?;
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(suffix))
}

fn display_path_for_prompt(path: &Path, cwd: &Path) -> String {
    path.strip_prefix(cwd)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_simple_at_file_reference() {
        let temp = tempfile::tempdir().expect("temp dir");
        let file = temp.path().join("note.txt");
        std::fs::write(&file, "hello from file").expect("write test file");

        let expanded = expand_at_file_references("Read @note.txt please", temp.path());

        assert!(expanded.text.contains("Contents of note.txt:"));
        assert!(expanded.text.contains("hello from file"));
        assert!(expanded.expanded_paths.contains(&"note.txt".to_string()));
        assert!(expanded.warnings.is_empty());
    }

    #[test]
    fn expands_quoted_at_file_reference_with_spaces() {
        let temp = tempfile::tempdir().expect("temp dir");
        let file = temp.path().join("my note.txt");
        std::fs::write(&file, "quoted path content").expect("write test file");

        let expanded = expand_at_file_references("Summarize @\"my note.txt\"", temp.path());

        assert!(expanded.text.contains("Contents of my note.txt:"));
        assert!(expanded.text.contains("quoted path content"));
    }

    #[test]
    fn expands_directory_as_tree() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(temp.path().join("dir/src")).expect("mkdirs");
        std::fs::write(temp.path().join("dir/Cargo.toml"), "[package]\nname='x'\n")
            .expect("write cargo");
        std::fs::write(temp.path().join("dir/src/lib.rs"), "pub fn hi() {}\n").expect("write lib");

        let expanded = expand_at_file_references("Explain @dir/", temp.path());

        assert!(expanded.text.contains("Directory tree for dir:"));
        assert!(expanded.text.contains("Cargo.toml"));
        assert!(expanded.text.contains("src/"));
        assert!(expanded.warnings.is_empty());
    }

    #[test]
    fn expands_large_rust_file_as_outline() {
        let temp = tempfile::tempdir().expect("temp dir");
        let file = temp.path().join("big.rs");
        let mut content = String::new();
        content.push_str("pub struct SessionStore {}\n");
        content.push_str("impl SessionStore {\n}");
        for idx in 0..260 {
            content.push_str(&format!("\npub fn item_{idx}() {{}}\n"));
        }
        std::fs::write(&file, content).expect("write large file");

        let expanded = expand_at_file_references("Explain @big.rs", temp.path());

        assert!(expanded.text.contains("File outline for big.rs:"));
        assert!(expanded.text.contains("struct SessionStore"));
        assert!(expanded.text.contains("fn item_0"));
        assert!(!expanded.text.contains("Contents of big.rs:"));
    }

    #[test]
    fn leaves_missing_file_reference_unchanged() {
        let temp = tempfile::tempdir().expect("temp dir");
        let expanded = expand_at_file_references("Check @missing.txt", temp.path());
        assert_eq!(expanded.text, "Check @missing.txt");
        assert!(expanded.expanded_paths.is_empty());
    }
}
