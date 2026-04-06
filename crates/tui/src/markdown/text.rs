use unicode_width::UnicodeWidthStr;

use super::*;

/// Strip ANSI escape codes to get visible text
pub(super) fn strip_ansi(s: &str) -> String {
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                i += 2;
                while i < bytes.len() {
                    let ch = s[i..].chars().next().expect("valid utf-8 after ansi csi");
                    i += ch.len_utf8();
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    let ch = s[i..].chars().next().expect("valid utf-8 after ansi osc");
                    i += ch.len_utf8();
                }
                continue;
            }
        }
        let ch = s[i..].chars().next().expect("valid utf-8 char");
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

/// Compute visible width of a string with ANSI codes
pub(super) fn visible_width(s: &str) -> usize {
    UnicodeWidthStr::width(strip_ansi(s).as_str())
}

/// Word-wrap text with ANSI codes to fit within `max_width`.
pub(super) fn word_wrap_ansi(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;

    // Track active ANSI codes so we can carry them across lines
    let mut active_codes: Vec<String> = Vec::new();

    // Split into segments: ANSI codes and visible text
    let segments = split_ansi_segments(text);

    for segment in segments {
        if segment.starts_with('\x1b') {
            // It's an ANSI escape code
            current.push_str(&segment);
            // Track it
            if segment.contains("[0m") {
                active_codes.clear();
            } else {
                active_codes.push(segment);
            }
            continue;
        }

        // Visible text - word wrap it
        for word in WordSplitter::new(&segment) {
            let word_w = UnicodeWidthStr::width(word);

            if current_width + word_w > max_width && current_width > 0 {
                // Wrap: close current line and start new one
                current.push_str(RESET);
                lines.push(current);
                current = active_codes.join("");

                // Skip leading space on new line
                let trimmed = word.trim_start();
                let trimmed_w = UnicodeWidthStr::width(trimmed);
                current.push_str(trimmed);
                current_width = trimmed_w;
            } else {
                current.push_str(word);
                current_width += word_w;
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

/// Split a string into ANSI escape segments and text segments.
fn split_ansi_segments(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if !current.is_empty() {
                segments.push(std::mem::take(&mut current));
            }

            let start = i;
            i += 1;
            if i < bytes.len() && bytes[i] == b'[' {
                i += 1;
                while i < bytes.len() {
                    let ch = text[i..].chars().next().expect("valid utf-8 in ansi csi");
                    i += ch.len_utf8();
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
                segments.push(text[start..i].to_string());
                continue;
            }
            if i < bytes.len() && bytes[i] == b']' {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    let ch = text[i..].chars().next().expect("valid utf-8 in ansi osc");
                    i += ch.len_utf8();
                }
                segments.push(text[start..i].to_string());
                continue;
            }
            current.push('\x1b');
            continue;
        }
        let ch = text[i..].chars().next().expect("valid utf-8 char");
        current.push(ch);
        i += ch.len_utf8();
    }

    if !current.is_empty() {
        segments.push(current);
    }

    segments
}

/// Helper to split text into words while preserving spaces as part of the word.
struct WordSplitter<'a> {
    remaining: &'a str,
}

impl<'a> WordSplitter<'a> {
    fn new(s: &'a str) -> Self {
        Self { remaining: s }
    }
}

impl<'a> Iterator for WordSplitter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }

        // Find next space boundary
        let bytes = self.remaining.as_bytes();
        // Find end of current chunk (non-space then space, or space then non-space)
        let mut i = 0;
        let starts_with_space = bytes[0] == b' ';

        if starts_with_space {
            // Consume all spaces
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
        } else {
            // Consume until space
            while i < bytes.len() && bytes[i] != b' ' {
                i += 1;
            }
        }

        let (chunk, rest) = self.remaining.split_at(i);
        self.remaining = rest;
        Some(chunk)
    }
}
