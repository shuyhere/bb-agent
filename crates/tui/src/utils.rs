//! ANSI-aware string utilities for terminal rendering.
//!
//! Ported from pi-tui's utils.ts. Handles:
//! - Visible width calculation (strips ANSI, handles CJK/emoji)
//! - Truncation to width (preserves ANSI codes)
//! - Padding to width
//! - ANSI code extraction and stripping

use unicode_width::UnicodeWidthChar;

/// Calculate the visible width of a string, excluding ANSI escape codes.
pub fn visible_width(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }

    // Fast path: pure ASCII printable, no escape codes
    if is_printable_ascii(s) {
        return s.len();
    }

    let stripped = strip_ansi(s);
    // Replace tabs with 3 spaces (matching pi behavior)
    let stripped = stripped.replace('\t', "   ");

    stripped.chars().map(|c| char_width(c)).sum()
}

/// Width of a single character in terminal columns.
fn char_width(c: char) -> usize {
    if c < ' ' {
        return 0; // control chars
    }
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// Check if string is pure printable ASCII (no escape codes, no unicode).
fn is_printable_ascii(s: &str) -> bool {
    s.bytes().all(|b| b >= 0x20 && b <= 0x7e)
}

/// Strip all ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if let Some(skip) = ansi_sequence_len(bytes, i) {
                i += skip;
                continue;
            }
        }
        // Safe: we're copying byte by byte from valid UTF-8
        result.push(s[i..].chars().next().unwrap());
        i += s[i..].chars().next().unwrap().len_utf8();
    }

    result
}

/// Determine the length of an ANSI escape sequence starting at `pos`.
fn ansi_sequence_len(bytes: &[u8], pos: usize) -> Option<usize> {
    if pos >= bytes.len() || bytes[pos] != 0x1b {
        return None;
    }
    if pos + 1 >= bytes.len() {
        return None;
    }

    match bytes[pos + 1] {
        // CSI: ESC [ ... <terminator>
        b'[' => {
            let mut j = pos + 2;
            while j < bytes.len() {
                let b = bytes[j];
                // CSI terminators: letters and a few special chars
                if b >= 0x40 && b <= 0x7e {
                    return Some(j + 1 - pos);
                }
                j += 1;
            }
            None
        }
        // OSC: ESC ] ... BEL or ESC ] ... ST
        b']' => {
            let mut j = pos + 2;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    return Some(j + 1 - pos);
                }
                if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                    return Some(j + 2 - pos);
                }
                j += 1;
            }
            None
        }
        // APC: ESC _ ... BEL or ESC _ ... ST
        b'_' => {
            let mut j = pos + 2;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    return Some(j + 1 - pos);
                }
                if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                    return Some(j + 2 - pos);
                }
                j += 1;
            }
            None
        }
        _ => None,
    }
}

/// Truncate a string to `max_width` visible columns, preserving ANSI codes.
/// Appends `…` if truncated.
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    let vw = visible_width(s);
    if vw <= max_width {
        return s.to_string();
    }

    if max_width == 0 {
        return String::new();
    }

    let mut result = String::new();
    let mut width = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    let target = if max_width > 1 { max_width - 1 } else { max_width }; // room for …

    while i < bytes.len() && width < target {
        // Skip ANSI sequences (include them in output but don't count width)
        if bytes[i] == 0x1b {
            if let Some(len) = ansi_sequence_len(bytes, i) {
                result.push_str(&s[i..i + len]);
                i += len;
                continue;
            }
        }

        let ch = s[i..].chars().next().unwrap();
        let cw = char_width(ch);
        if width + cw > target {
            break;
        }
        result.push(ch);
        width += cw;
        i += ch.len_utf8();
    }

    result.push_str("\x1b[0m…");
    result
}

/// Pad a string to exactly `width` visible columns with trailing spaces.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let vw = visible_width(s);
    if vw >= width {
        return s.to_string();
    }
    format!("{}{}", s, " ".repeat(width - vw))
}

/// Wrap text to `max_width`, preserving words when possible.
/// Returns a Vec of lines. Handles ANSI codes (they pass through).
pub fn word_wrap(s: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![s.to_string()];
    }

    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;

    for word in s.split_inclusive(' ') {
        let word_width = visible_width(word);

        if current_width + word_width <= max_width {
            current_line.push_str(word);
            current_width += word_width;
        } else if current_width == 0 {
            // Word is longer than max_width, force it on its own line
            current_line.push_str(word);
            current_width += word_width;
        } else {
            // Start new line
            lines.push(current_line.trim_end().to_string());
            current_line = word.to_string();
            current_width = word_width;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line.trim_end().to_string());
    }
    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visible_width_plain() {
        assert_eq!(visible_width("hello"), 5);
        assert_eq!(visible_width(""), 0);
        assert_eq!(visible_width("abc"), 3);
    }

    #[test]
    fn test_visible_width_ansi() {
        assert_eq!(visible_width("\x1b[31mhello\x1b[0m"), 5);
        assert_eq!(visible_width("\x1b[1;32mbold green\x1b[0m"), 10);
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
        assert_eq!(strip_ansi("no codes"), "no codes");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
        assert_eq!(visible_width(&truncate_to_width("hello world!", 5)), 5);
    }

    #[test]
    fn test_truncate_with_ansi() {
        let s = "\x1b[31mhello world\x1b[0m";
        let t = truncate_to_width(s, 5);
        assert!(t.contains("\x1b[31m")); // ANSI preserved
        assert_eq!(visible_width(&t), 5);
    }

    #[test]
    fn test_pad() {
        assert_eq!(pad_to_width("hi", 5), "hi   ");
        assert_eq!(pad_to_width("hello", 5), "hello");
    }

    #[test]
    fn test_word_wrap() {
        let lines = word_wrap("hello world foo bar", 10);
        assert!(lines.len() >= 2);
        for line in &lines {
            assert!(visible_width(line) <= 10);
        }
    }

    #[test]
    fn test_word_wrap_long_word() {
        let lines = word_wrap("superlongword", 5);
        assert_eq!(lines.len(), 1); // can't break, stays on one line
    }
}
