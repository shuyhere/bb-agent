use unicode_width::UnicodeWidthChar;

/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Consume ESC [ ... final_byte
            if let Some(next) = chars.next() {
                if next == '[' {
                    // CSI sequence: consume until 0x40-0x7E
                    for c2 in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c2) {
                            break;
                        }
                    }
                } else if next == ']' {
                    // OSC sequence: consume until ST (ESC \ or BEL)
                    let mut prev = next;
                    for c2 in chars.by_ref() {
                        if c2 == '\x07' || (prev == '\x1b' && c2 == '\\') {
                            break;
                        }
                        prev = c2;
                    }
                }
                // else: skip the two chars
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Calculate visible width of a string, ignoring ANSI escape sequences.
/// Uses unicode-width for correct CJK/emoji handling.
pub fn visible_width(s: &str) -> usize {
    let stripped = strip_ansi(s);
    stripped.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
}

/// Truncate a string to fit within `max` visible columns.
/// Preserves ANSI escape sequences but truncates visible characters.
pub fn truncate_to_width(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut width = 0usize;
    let mut chars = s.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c == '\x1b' {
            // Pass through entire escape sequence
            chars.next();
            out.push(c);
            if let Some(&next) = chars.peek() {
                if next == '[' {
                    chars.next();
                    out.push(next);
                    while let Some(&c2) = chars.peek() {
                        chars.next();
                        out.push(c2);
                        if ('\x40'..='\x7e').contains(&c2) {
                            break;
                        }
                    }
                } else if next == ']' {
                    chars.next();
                    out.push(next);
                    let mut prev = next;
                    while let Some(&c2) = chars.peek() {
                        chars.next();
                        out.push(c2);
                        if c2 == '\x07' || (prev == '\x1b' && c2 == '\\') {
                            break;
                        }
                        prev = c2;
                    }
                } else {
                    chars.next();
                    out.push(next);
                }
            }
        } else {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if width + cw > max {
                break;
            }
            chars.next();
            out.push(c);
            width += cw;
        }
    }
    out
}

/// Pad a string with spaces so its visible width equals `width`.
/// If already wider, returns as-is.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let vw = visible_width(s);
    if vw >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - vw))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visible_width_plain() {
        assert_eq!(visible_width("hello"), 5);
    }

    #[test]
    fn test_visible_width_ansi() {
        assert_eq!(visible_width("\x1b[31mhello\x1b[0m"), 5);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate_to_width("hello world", 5), "hello");
    }

    #[test]
    fn test_truncate_with_ansi() {
        let s = "\x1b[31mhello\x1b[0m world";
        let t = truncate_to_width(s, 5);
        assert_eq!(visible_width(&t), 5);
    }

    #[test]
    fn test_pad() {
        assert_eq!(pad_to_width("hi", 5), "hi   ");
    }
}
