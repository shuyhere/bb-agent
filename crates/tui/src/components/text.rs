use std::cell::RefCell;

use crate::{component::Component, impl_as_any, utils::visible_width};

pub type CustomBgFn = Box<dyn Fn(&str) -> String + Send>;

#[derive(Default)]
struct RenderCache {
    text: Option<String>,
    width: Option<u16>,
    lines: Option<Vec<String>>,
}

/// Text component - displays multi-line text with word wrapping.
pub struct Text {
    text: String,
    padding_x: usize,
    padding_y: usize,
    custom_bg_fn: Option<CustomBgFn>,
    cache: RefCell<RenderCache>,
}

impl Text {
    pub fn new(
        text: impl Into<String>,
        padding_x: usize,
        padding_y: usize,
        custom_bg_fn: Option<CustomBgFn>,
    ) -> Self {
        Self {
            text: text.into(),
            padding_x,
            padding_y,
            custom_bg_fn,
            cache: RefCell::new(RenderCache::default()),
        }
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.invalidate();
    }

    pub fn set_custom_bg_fn(&mut self, custom_bg_fn: Option<CustomBgFn>) {
        self.custom_bg_fn = custom_bg_fn;
        self.invalidate();
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::new("", 1, 1, None)
    }
}

impl Component for Text {
    fn render(&self, width: u16) -> Vec<String> {
        {
            let cache = self.cache.borrow();
            if let Some(lines) = &cache.lines
                && cache.text.as_deref() == Some(self.text.as_str())
                && cache.width == Some(width)
            {
                return lines.clone();
            }
        }

        if self.text.trim().is_empty() {
            let result = Vec::new();
            *self.cache.borrow_mut() = RenderCache {
                text: Some(self.text.clone()),
                width: Some(width),
                lines: Some(result.clone()),
            };
            return result;
        }

        let width_usize = usize::from(width);
        let normalized_text = self.text.replace('\t', "   ");
        let content_width = width_usize
            .saturating_sub(self.padding_x.saturating_mul(2))
            .max(1);
        let wrapped_lines = wrap_text_with_ansi(&normalized_text, content_width);

        let left_margin = " ".repeat(self.padding_x);
        let right_margin = " ".repeat(self.padding_x);
        let mut content_lines = Vec::with_capacity(wrapped_lines.len());

        for line in wrapped_lines {
            let line_with_margins = format!("{left_margin}{line}{right_margin}");
            if let Some(custom_bg_fn) = &self.custom_bg_fn {
                content_lines.push(apply_background_to_line(
                    &line_with_margins,
                    width_usize,
                    custom_bg_fn.as_ref(),
                ));
            } else {
                let visible_len = visible_width(&line_with_margins);
                let padding_needed = width_usize.saturating_sub(visible_len);
                content_lines.push(format!("{line_with_margins}{}", " ".repeat(padding_needed)));
            }
        }

        let empty_line = " ".repeat(width_usize);
        let mut empty_lines = Vec::with_capacity(self.padding_y);
        for _ in 0..self.padding_y {
            let line = if let Some(custom_bg_fn) = &self.custom_bg_fn {
                apply_background_to_line(&empty_line, width_usize, custom_bg_fn.as_ref())
            } else {
                empty_line.clone()
            };
            empty_lines.push(line);
        }

        let mut result = Vec::with_capacity(empty_lines.len() * 2 + content_lines.len());
        result.extend(empty_lines.iter().cloned());
        result.extend(content_lines);
        result.extend(empty_lines.iter().cloned());

        *self.cache.borrow_mut() = RenderCache {
            text: Some(self.text.clone()),
            width: Some(width),
            lines: Some(result.clone()),
        };

        if result.is_empty() {
            vec![String::new()]
        } else {
            result
        }
    }

    fn invalidate(&mut self) {
        *self.cache.get_mut() = RenderCache::default();
    }

    impl_as_any!();
}

fn apply_background_to_line(line: &str, width: usize, bg_fn: &dyn Fn(&str) -> String) -> String {
    let visible_len = visible_width(line);
    let padding_needed = width.saturating_sub(visible_len);
    let with_padding = format!("{line}{}", " ".repeat(padding_needed));
    bg_fn(&with_padding)
}

fn wrap_text_with_ansi(text: &str, width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let input_lines: Vec<&str> = text.split('\n').collect();
    let mut result = Vec::new();
    let mut tracker = AnsiCodeTracker::default();

    for input_line in input_lines {
        let prefix = if result.is_empty() {
            String::new()
        } else {
            tracker.get_active_codes()
        };
        result.extend(wrap_single_line(&(prefix + input_line), width));
        update_tracker_from_text(input_line, &mut tracker);
    }

    if result.is_empty() {
        vec![String::new()]
    } else {
        result
    }
}

fn wrap_single_line(line: &str, width: usize) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let visible_length = visible_width(line);
    if visible_length <= width {
        return vec![line.to_string()];
    }

    let mut wrapped = Vec::new();
    let mut tracker = AnsiCodeTracker::default();
    let tokens = split_into_tokens_with_ansi(line);

    let mut current_line = String::new();
    let mut current_visible_length = 0;

    for token in tokens {
        let token_visible_length = visible_width(&token);
        let is_whitespace = token.trim().is_empty();

        if token_visible_length > width && !is_whitespace {
            if !current_line.is_empty() {
                let line_end_reset = tracker.get_line_end_reset();
                if !line_end_reset.is_empty() {
                    current_line.push_str(&line_end_reset);
                }
                wrapped.push(current_line);
            }

            let broken = break_long_word(&token, width, &mut tracker);
            let broken_len = broken.len();
            if broken_len > 1 {
                wrapped.extend(broken[..broken_len - 1].iter().cloned());
            }
            current_line = broken.last().cloned().unwrap_or_default();
            current_visible_length = visible_width(&current_line);
            continue;
        }

        let total_needed = current_visible_length + token_visible_length;
        if total_needed > width && current_visible_length > 0 {
            let mut line_to_wrap = current_line.trim_end().to_string();
            let line_end_reset = tracker.get_line_end_reset();
            if !line_end_reset.is_empty() {
                line_to_wrap.push_str(&line_end_reset);
            }
            wrapped.push(line_to_wrap);

            if is_whitespace {
                current_line = tracker.get_active_codes();
                current_visible_length = 0;
            } else {
                current_line = tracker.get_active_codes() + &token;
                current_visible_length = token_visible_length;
            }
        } else {
            current_line.push_str(&token);
            current_visible_length += token_visible_length;
        }

        update_tracker_from_text(&token, &mut tracker);
    }

    if !current_line.is_empty() {
        wrapped.push(current_line);
    }

    if wrapped.is_empty() {
        vec![String::new()]
    } else {
        wrapped
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect()
    }
}

fn break_long_word(word: &str, width: usize, tracker: &mut AnsiCodeTracker) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = tracker.get_active_codes();
    let mut current_width = 0;

    let mut i = 0;
    let mut segments: Vec<Segment> = Vec::new();
    while i < word.len() {
        if let Some(ansi) = extract_ansi_code(word, i) {
            segments.push(Segment::Ansi(ansi.code));
            i += ansi.length;
            continue;
        }

        let ch = next_char(word, i);
        segments.push(Segment::Grapheme(ch.to_string()));
        i += ch.len_utf8();
    }

    for segment in segments {
        match segment {
            Segment::Ansi(code) => {
                current_line.push_str(&code);
                tracker.process(&code);
            }
            Segment::Grapheme(grapheme) => {
                if grapheme.is_empty() {
                    continue;
                }

                let grapheme_width = visible_width(&grapheme);
                if current_width + grapheme_width > width {
                    let line_end_reset = tracker.get_line_end_reset();
                    if !line_end_reset.is_empty() {
                        current_line.push_str(&line_end_reset);
                    }
                    lines.push(current_line);
                    current_line = tracker.get_active_codes();
                    current_width = 0;
                }

                current_line.push_str(&grapheme);
                current_width += grapheme_width;
            }
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn update_tracker_from_text(text: &str, tracker: &mut AnsiCodeTracker) {
    let mut i = 0;
    while i < text.len() {
        if let Some(ansi) = extract_ansi_code(text, i) {
            tracker.process(&ansi.code);
            i += ansi.length;
        } else {
            i += next_char(text, i).len_utf8();
        }
    }
}

fn split_into_tokens_with_ansi(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut pending_ansi = String::new();
    let mut in_whitespace = false;
    let mut i = 0;

    while i < text.len() {
        if let Some(ansi) = extract_ansi_code(text, i) {
            pending_ansi.push_str(&ansi.code);
            i += ansi.length;
            continue;
        }

        let ch = next_char(text, i);
        let char_is_space = ch == ' ';

        if char_is_space != in_whitespace && !current.is_empty() {
            tokens.push(current);
            current = String::new();
        }

        if !pending_ansi.is_empty() {
            current.push_str(&pending_ansi);
            pending_ansi.clear();
        }

        in_whitespace = char_is_space;
        current.push(ch);
        i += ch.len_utf8();
    }

    if !pending_ansi.is_empty() {
        current.push_str(&pending_ansi);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[derive(Default)]
struct AnsiCodeTracker {
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    blink: bool,
    inverse: bool,
    hidden: bool,
    strikethrough: bool,
    fg_color: Option<String>,
    bg_color: Option<String>,
}

impl AnsiCodeTracker {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn process(&mut self, ansi_code: &str) {
        if !ansi_code.ends_with('m') {
            return;
        }

        let params = ansi_code
            .strip_prefix("\x1b[")
            .and_then(|s| s.strip_suffix('m'));
        let Some(params) = params else {
            return;
        };

        if params.is_empty() || params == "0" {
            self.reset();
            return;
        }

        let parts: Vec<&str> = params.split(';').collect();
        let mut i = 0;
        while i < parts.len() {
            let code = parts[i].parse::<u16>().unwrap_or(0);

            if code == 38 || code == 48 {
                if parts.get(i + 1) == Some(&"5") && parts.get(i + 2).is_some() {
                    let color_code = format!("{};{};{}", parts[i], parts[i + 1], parts[i + 2]);
                    if code == 38 {
                        self.fg_color = Some(color_code);
                    } else {
                        self.bg_color = Some(color_code);
                    }
                    i += 3;
                    continue;
                } else if parts.get(i + 1) == Some(&"2") && parts.get(i + 4).is_some() {
                    let color_code = format!(
                        "{};{};{};{};{}",
                        parts[i],
                        parts[i + 1],
                        parts[i + 2],
                        parts[i + 3],
                        parts[i + 4]
                    );
                    if code == 38 {
                        self.fg_color = Some(color_code);
                    } else {
                        self.bg_color = Some(color_code);
                    }
                    i += 5;
                    continue;
                }
            }

            match code {
                0 => self.reset(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                5 => self.blink = true,
                7 => self.inverse = true,
                8 => self.hidden = true,
                9 => self.strikethrough = true,
                21 => self.bold = false,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                25 => self.blink = false,
                27 => self.inverse = false,
                28 => self.hidden = false,
                29 => self.strikethrough = false,
                39 => self.fg_color = None,
                49 => self.bg_color = None,
                30..=37 | 90..=97 => self.fg_color = Some(code.to_string()),
                40..=47 | 100..=107 => self.bg_color = Some(code.to_string()),
                _ => {}
            }

            i += 1;
        }
    }

    fn get_active_codes(&self) -> String {
        let mut codes: Vec<String> = Vec::new();
        if self.bold {
            codes.push("1".to_string());
        }
        if self.dim {
            codes.push("2".to_string());
        }
        if self.italic {
            codes.push("3".to_string());
        }
        if self.underline {
            codes.push("4".to_string());
        }
        if self.blink {
            codes.push("5".to_string());
        }
        if self.inverse {
            codes.push("7".to_string());
        }
        if self.hidden {
            codes.push("8".to_string());
        }
        if self.strikethrough {
            codes.push("9".to_string());
        }
        if let Some(fg) = &self.fg_color {
            codes.push(fg.clone());
        }
        if let Some(bg) = &self.bg_color {
            codes.push(bg.clone());
        }

        if codes.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", codes.join(";"))
        }
    }

    fn get_line_end_reset(&self) -> String {
        if self.underline {
            "\x1b[24m".to_string()
        } else {
            String::new()
        }
    }
}

enum Segment {
    Ansi(String),
    Grapheme(String),
}

struct AnsiCode {
    code: String,
    length: usize,
}

fn extract_ansi_code(s: &str, pos: usize) -> Option<AnsiCode> {
    let bytes = s.as_bytes();
    if pos >= bytes.len() || bytes[pos] != 0x1b || pos + 1 >= bytes.len() {
        return None;
    }

    match bytes[pos + 1] {
        b'[' => {
            let mut j = pos + 2;
            while j < bytes.len() {
                let b = bytes[j];
                if matches!(b, b'm' | b'G' | b'K' | b'H' | b'J') {
                    return Some(AnsiCode {
                        code: s[pos..=j].to_string(),
                        length: j + 1 - pos,
                    });
                }
                j += 1;
            }
            None
        }
        b']' | b'_' => {
            let mut j = pos + 2;
            while j < bytes.len() {
                if bytes[j] == 0x07 {
                    return Some(AnsiCode {
                        code: s[pos..=j].to_string(),
                        length: j + 1 - pos,
                    });
                }
                if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                    return Some(AnsiCode {
                        code: s[pos..j + 2].to_string(),
                        length: j + 2 - pos,
                    });
                }
                j += 1;
            }
            None
        }
        _ => None,
    }
}

fn next_char(s: &str, index: usize) -> char {
    s[index..].chars().next().expect("valid char boundary")
}
