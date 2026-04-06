//! Syntax highlighting for file content using syntect.
//!
//! Provides a shared highlighter that maps file extensions to languages and
//! produces ANSI-colored output lines.

use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

const RESET: &str = "\x1b[0m";

/// Map file extension to syntect language token.
pub fn language_from_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?.to_lowercase();
    let lang = match ext.as_str() {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "py" => "python",
        "rb" => "ruby",
        "go" => "go",
        "java" => "java",
        "kt" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "sh" | "bash" | "zsh" => "bash",
        "sql" => "sql",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "md" | "markdown" => "markdown",
        "dockerfile" => "dockerfile",
        "makefile" => "makefile",
        "lua" => "lua",
        "r" => "r",
        "scala" => "scala",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "hs" => "haskell",
        "ml" | "mli" => "ocaml",
        "fs" | "fsi" | "fsx" => "fsharp",
        "clj" | "cljs" | "cljc" => "clojure",
        "proto" => "protobuf",
        "graphql" | "gql" => "graphql",
        "tf" | "hcl" => "hcl",
        "tex" | "latex" => "latex",
        "zig" => "zig",
        "nim" => "nim",
        "dart" => "dart",
        "v" => "vlang",
        "jl" => "julia",
        "pl" | "pm" => "perl",
        "groovy" | "gvy" => "groovy",
        "ps1" | "psm1" => "powershell",
        _ => return None,
    };
    Some(lang)
}

/// Syntax-highlight a block of code, returning one ANSI-colored string per line.
/// Falls back to plain (themed gray) output if the language is unknown.
pub fn highlight_code(code: &str, lang: Option<&str>) -> Vec<String> {
    let syntax = lang
        .and_then(|l| SYNTAX_SET.find_syntax_by_token(l))
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    let theme = &THEME_SET.themes["base16-ocean.dark"];
    let mut hl = HighlightLines::new(syntax, theme);

    code.lines()
        .map(|line| {
            let regions = hl.highlight_line(line, &SYNTAX_SET).unwrap_or_default();
            let mut out = String::new();
            for (style, text) in regions {
                let fg = style.foreground;
                out.push_str(&format!("\x1b[38;2;{};{};{}m", fg.r, fg.g, fg.b));
                if style.font_style.contains(FontStyle::BOLD) {
                    out.push_str("\x1b[1m");
                }
                if style.font_style.contains(FontStyle::ITALIC) {
                    out.push_str("\x1b[3m");
                }
                out.push_str(text);
                out.push_str(RESET);
            }
            out
        })
        .collect()
}
