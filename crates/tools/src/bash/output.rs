use regex::{Captures, Regex};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::artifacts;

const MAX_OUTPUT_LINES: usize = 2000;
const MAX_OUTPUT_BYTES: usize = 50 * 1024;

/// Buffers partial shell output until a line boundary is available so live redaction never
/// emits incomplete secret fragments split across stdout/stderr chunks.
#[derive(Default)]
pub(super) struct BashOutputRedactor {
    pending: String,
}

impl BashOutputRedactor {
    pub(super) fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let Some(flush_at) = self.last_line_boundary() else {
            return String::new();
        };
        let stable: String = self.pending.drain(..flush_at).collect();
        redact_bash_output_text(&stable)
    }

    pub(super) fn finish(&mut self) -> String {
        let tail = std::mem::take(&mut self.pending);
        redact_bash_output_text(&tail)
    }

    fn last_line_boundary(&self) -> Option<usize> {
        self.pending
            .char_indices()
            .filter_map(|(idx, ch)| match ch {
                '\n' | '\r' => Some(idx + ch.len_utf8()),
                _ => None,
            })
            .next_back()
    }
}

fn is_secret_name(name: &str) -> bool {
    let upper = name.replace('-', "_").to_ascii_uppercase();
    upper.contains("API_KEY")
        || upper == "APIKEY"
        || upper == "TOKEN"
        || upper.ends_with("_TOKEN")
        || upper.contains("ACCESS_TOKEN")
        || upper.contains("REFRESH_TOKEN")
        || upper.contains("SECRET")
        || upper.contains("PASSWORD")
        || upper.contains("PASSWD")
}

fn secret_assignment_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b([A-Za-z_][A-Za-z0-9_]*)=([^\s]+)").expect("valid regex"))
}

fn secret_field_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?i)(["']?(?:api[_-]?key|apikey|access[_-]?token|refresh[_-]?token|token|password|passwd|secret)["']?\s*[:=]\s*["']?)([^"'\s,;&]+)(["']?)"#,
        )
        .expect("valid regex")
    })
}

fn authorization_header_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)Authorization:\s*(Bearer|token)\s+[^\s"']+"#).expect("valid regex")
    })
}

fn bearer_token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)\bBearer\s+[^\s"']+"#).expect("valid regex"))
}

pub(super) fn redact_bash_output_text(text: &str) -> String {
    let redacted_assignments = secret_assignment_regex().replace_all(text, |caps: &Captures| {
        let name = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        if is_secret_name(name) {
            format!("{name}=[REDACTED]")
        } else {
            caps.get(0)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        }
    });
    let redacted_fields =
        secret_field_regex().replace_all(redacted_assignments.as_ref(), |caps: &Captures| {
            let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
            let suffix = caps.get(3).map(|m| m.as_str()).unwrap_or_default();
            format!("{prefix}[REDACTED]{suffix}")
        });
    let redacted_auth =
        authorization_header_regex().replace_all(redacted_fields.as_ref(), |caps: &Captures| {
            let scheme = caps.get(1).map(|m| m.as_str()).unwrap_or("Bearer");
            format!("Authorization: {scheme} [REDACTED]")
        });
    bearer_token_regex()
        .replace_all(redacted_auth.as_ref(), "Bearer [REDACTED]")
        .into_owned()
}

pub(super) struct StoredBashOutput {
    pub output: String,
    pub artifact_path: Option<PathBuf>,
    pub truncated: bool,
}

/// Applies the same truncation policy used by the public tool contract: offload large payloads
/// by byte size first, otherwise keep the output inline and cap it by rendered line count.
pub(super) fn store_bash_output(output: &str, artifacts_dir: &Path) -> StoredBashOutput {
    let mut truncated = false;
    let (output, artifact_path) =
        artifacts::maybe_offload(output, artifacts_dir, Some(MAX_OUTPUT_BYTES));
    if artifact_path.is_some() {
        truncated = true;
        return StoredBashOutput {
            output,
            artifact_path,
            truncated,
        };
    }

    let lines: Vec<&str> = output.lines().collect();
    if lines.len() > MAX_OUTPUT_LINES {
        let joined = lines[..MAX_OUTPUT_LINES].join("\n");
        let remaining = lines.len() - MAX_OUTPUT_LINES;
        truncated = true;
        return StoredBashOutput {
            output: format!("{joined}\n\n[{remaining} more lines truncated]"),
            artifact_path: None,
            truncated,
        };
    }

    StoredBashOutput {
        output,
        artifact_path: None,
        truncated,
    }
}
