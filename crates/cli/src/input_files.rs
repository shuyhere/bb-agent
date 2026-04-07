use std::path::{Path, PathBuf};

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
            if let Ok(content) = std::fs::read_to_string(&resolved) {
                let display_path = display_path_for_prompt(&resolved, cwd);
                out.push_str(&format!("Contents of {display_path}:\n```\n{content}\n```"));
                expanded_paths.push(display_path);
                cursor = end;
                continue;
            }

            if resolved.exists() {
                warnings.push(format!(
                    "Could not read {} as UTF-8 text",
                    display_path_for_prompt(&resolved, cwd)
                ));
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
    fn leaves_missing_file_reference_unchanged() {
        let temp = tempfile::tempdir().expect("temp dir");
        let expanded = expand_at_file_references("Check @missing.txt", temp.path());
        assert_eq!(expanded.text, "Check @missing.txt");
        assert!(expanded.expanded_paths.is_empty());
    }
}
