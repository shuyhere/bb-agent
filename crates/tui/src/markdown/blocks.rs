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
    let sample: String = text.chars().take(1000).collect();

    if sample.contains("```")
        || sample.contains("**")
        || sample.contains("__")
        || sample.contains("~~")
        || sample.contains('`')
        || (sample.contains('[')
            && sample.contains(']')
            && (sample.contains("](") || sample.contains("][") || sample.contains("]:")))
    {
        return true;
    }

    let mut previous_non_empty: Option<&str> = None;
    for line in sample.lines() {
        let trimmed = line.trim();
        let trimmed_start = line.trim_start();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed_start.starts_with("```")
            || trimmed_start.starts_with("> ")
            || trimmed_start.starts_with("- ")
            || trimmed_start.starts_with("* ")
            || trimmed_start.starts_with("+ ")
            || trimmed_start.starts_with("- [ ] ")
            || trimmed_start.starts_with("- [x] ")
            || trimmed_start.starts_with("- [X] ")
        {
            return true;
        }

        if trimmed_start.starts_with('#') {
            let hashes = trimmed_start.chars().take_while(|&ch| ch == '#').count();
            if (1..=6).contains(&hashes)
                && trimmed_start
                    .chars()
                    .nth(hashes)
                    .is_some_and(|ch| ch.is_whitespace())
            {
                return true;
            }
        }

        let mut seen_digit = false;
        let mut chars = trimmed_start.chars().peekable();
        while let Some(ch) = chars.peek().copied() {
            if ch.is_ascii_digit() {
                seen_digit = true;
                chars.next();
            } else {
                break;
            }
        }
        if seen_digit && chars.next() == Some('.') && chars.next() == Some(' ') {
            return true;
        }

        if is_horizontal_rule(trimmed) {
            return true;
        }

        if let Some(prev) = previous_non_empty
            && (is_setext_underline(trimmed, '=') || is_setext_underline(trimmed, '-'))
            && !prev.trim().is_empty()
        {
            return true;
        }

        if trimmed.starts_with('|') || trimmed.ends_with('|') || trimmed.contains(" | ") {
            return true;
        }

        previous_non_empty = Some(line);
    }

    false
}

fn is_horizontal_rule(line: &str) -> bool {
    let mut marker = None;
    let mut count = 0usize;

    for ch in line.chars() {
        if ch.is_whitespace() {
            continue;
        }
        if !matches!(ch, '-' | '*' | '_') {
            return false;
        }
        match marker {
            Some(existing) if existing != ch => return false,
            Some(_) => {}
            None => marker = Some(ch),
        }
        count += 1;
    }

    count >= 3
}

fn is_setext_underline(line: &str, marker: char) -> bool {
    let trimmed = line.trim();
    trimmed.len() >= 3 && trimmed.chars().all(|ch| ch == marker)
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
