use super::cache::{get_cached_tokens, put_cached_tokens, text_hash};
use super::github::auto_link_github_refs;
use super::text::{strip_ansi, word_wrap_ansi};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MarkdownBlockToken {
    pub(super) text: String,
}

pub(super) fn markdown_block_tokens(text: &str) -> Vec<MarkdownBlockToken> {
    let key = text_hash(text);
    if let Some(tokens) = get_cached_tokens(key) {
        return tokens;
    }

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_fence = false;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let is_fence = trimmed.trim_start().starts_with("```");
        if is_fence {
            in_fence = !in_fence;
            current.push_str(line);
            continue;
        }

        if !in_fence && trimmed.trim().is_empty() {
            if !current.trim().is_empty() {
                tokens.push(MarkdownBlockToken {
                    text: current.trim_end_matches('\n').to_string(),
                });
                current.clear();
            }
            continue;
        }

        current.push_str(line);
    }

    if !current.trim().is_empty() {
        tokens.push(MarkdownBlockToken {
            text: current.trim_end_matches('\n').to_string(),
        });
    }

    put_cached_tokens(key, tokens.clone());
    tokens
}

pub(super) fn stable_token_count(text: &str, tokens: &[MarkdownBlockToken]) -> usize {
    if tokens.is_empty() {
        0
    } else if ends_on_stable_boundary(text) {
        tokens.len()
    } else {
        tokens.len().saturating_sub(1)
    }
}

pub(super) fn trim_blank_edges(mut lines: Vec<String>) -> Vec<String> {
    while lines
        .first()
        .is_some_and(|line| strip_ansi(line).trim().is_empty())
    {
        lines.remove(0);
    }
    while lines
        .last()
        .is_some_and(|line| strip_ansi(line).trim().is_empty())
    {
        lines.pop();
    }
    lines
}

pub(super) fn has_markdown_syntax(text: &str) -> bool {
    let sample: String = text.chars().take(500).collect();
    sample.contains("\n\n")
        || sample
            .chars()
            .any(|ch| matches!(ch, '#' | '*' | '`' | '|' | '[' | '>' | '-' | '_'))
        || sample.lines().any(|line| {
            let trimmed = line.trim_start();
            let mut seen_digit = false;
            let mut chars = trimmed.chars().peekable();
            while let Some(ch) = chars.peek().copied() {
                if ch.is_ascii_digit() {
                    seen_digit = true;
                    chars.next();
                } else {
                    break;
                }
            }
            seen_digit && chars.next() == Some('.') && chars.next() == Some(' ')
        })
}

pub(super) fn render_plain_text(text: &str, width: u16) -> Vec<String> {
    let width = width as usize;
    let mut out = Vec::new();
    for line in text.split('\n') {
        out.extend(word_wrap_ansi(&auto_link_github_refs(line), width.max(1)));
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn ends_on_stable_boundary(text: &str) -> bool {
    let trimmed_end = text.trim_end_matches([' ', '\t']);
    trimmed_end.ends_with("\n\n") || trimmed_end.ends_with("```")
}
