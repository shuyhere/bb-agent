#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderDiffOptions {
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedDiffLine {
    prefix: char,
    line_num: String,
    content: String,
}

fn parse_diff_line(line: &str) -> Option<ParsedDiffLine> {
    let mut chars = line.chars();
    let prefix = chars.next()?;
    if !matches!(prefix, '+' | '-' | ' ') {
        return None;
    }
    let rest = chars.as_str();
    let split = rest.find(' ')?;
    let (line_num, content) = rest.split_at(split);
    Some(ParsedDiffLine {
        prefix,
        line_num: line_num.trim().to_string(),
        content: content.trim_start().to_string(),
    })
}

fn replace_tabs(text: &str) -> String {
    text.replace('\t', "   ")
}

pub fn render_diff(diff_text: &str, _options: RenderDiffOptions) -> String {
    render_diff_lines(diff_text).join("\n")
}

pub fn render_diff_lines(diff_text: &str) -> Vec<String> {
    let mut lines = Vec::new();

    for line in diff_text.lines() {
        match parse_diff_line(line) {
            Some(parsed) => {
                let content = replace_tabs(&parsed.content);
                match parsed.prefix {
                    '+' => lines.push(format!("+{} {}", parsed.line_num, content)),
                    '-' => lines.push(format!("-{} {}", parsed.line_num, content)),
                    ' ' => lines.push(format!(" {} {}", parsed.line_num, content)),
                    _ => lines.push(line.to_string()),
                }
            }
            None => lines.push(line.to_string()),
        }
    }

    lines
}
