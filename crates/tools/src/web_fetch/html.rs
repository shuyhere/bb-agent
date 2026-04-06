use super::*;

static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<[^>]+>"#).expect("valid html tag regex"));
static SCRIPT_STYLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<(?:script|style|noscript|svg|canvas|iframe)\b[^>]*>.*?</(?:script|style|noscript|svg|canvas|iframe)>"#)
        .expect("valid script/style regex")
});
static BOILERPLATE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<(?:header|footer|nav|aside|form)\b[^>]*>.*?</(?:header|footer|nav|aside|form)>"#,
    )
    .expect("valid boilerplate regex")
});
static COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<!--.*?-->"#).expect("valid comment regex"));
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<title[^>]*>(.*?)</title>"#).expect("valid title regex"));
static MAIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<main\b[^>]*>(.*?)</main>"#).expect("valid main regex"));
static ARTICLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<article\b[^>]*>(.*?)</article>"#).expect("valid article regex")
});
static CONTENTISH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<(?:div|section)\b[^>]*(?:id|class)=['"][^'"]*(?:content|article|main|post|entry|markdown|doc|body|text)[^'"]*['"][^>]*>(.*?)</(?:div|section)>"#)
        .expect("valid contentish regex")
});
static BODY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<body\b[^>]*>(.*?)</body>"#).expect("valid body regex"));
static BR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<br\s*/?>"#).expect("valid br regex"));
static LI_OPEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<li\b[^>]*>"#).expect("valid li regex"));
static BLOCK_BREAK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)</(?:p|div|section|article|main|li|ul|ol|pre|blockquote|table|tr|h1|h2|h3|h4|h5|h6)>|<(?:p|div|section|article|main|ul|ol|pre|blockquote|table|tr|h1|h2|h3|h4|h5|h6)\b[^>]*>"#)
        .expect("valid block break regex")
});
static SPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[ \t\x0B\x0C\r]+"#).expect("valid whitespace regex"));
static MANY_NEWLINES_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\n{3,}"#).expect("valid newline regex"));

pub(crate) fn extract_title(html: &str) -> Option<String> {
    TITLE_RE
        .captures(html)
        .and_then(|caps| caps.get(1).map(|m| m.as_str()))
        .map(|title| decode_html_entities(&normalize_whitespace(title)))
        .filter(|title| !title.is_empty())
}

pub(crate) fn extract_main_content_text(html: &str) -> (String, &'static str) {
    let cleaned = clean_html_for_extraction(html);
    let candidates = [
        (
            "main",
            MAIN_RE
                .captures(&cleaned)
                .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string())),
        ),
        (
            "article",
            ARTICLE_RE
                .captures(&cleaned)
                .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string())),
        ),
        (
            "contentish",
            CONTENTISH_RE
                .captures(&cleaned)
                .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string())),
        ),
        (
            "body",
            BODY_RE
                .captures(&cleaned)
                .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string())),
        ),
    ];

    let mut best_source = "document";
    let mut best_text = html_to_text(&cleaned);
    let mut best_score = best_text.chars().count();

    for (source, candidate_html) in candidates {
        let Some(candidate_html) = candidate_html else {
            continue;
        };
        let text = html_to_text(&candidate_html);
        let score = text.chars().count();
        if source == "body" && best_source == "document" && score > 0 {
            best_source = source;
            best_text = text;
            best_score = score;
            continue;
        }
        if score > best_score / 3 && score > 40 {
            best_source = source;
            best_text = text;
            break;
        }
        if score > best_score {
            best_source = source;
            best_text = text;
            best_score = score;
        }
    }

    (best_text, best_source)
}

fn clean_html_for_extraction(html: &str) -> String {
    let no_comments = COMMENT_RE.replace_all(html, " ");
    let no_scripts = SCRIPT_STYLE_RE.replace_all(&no_comments, " ");
    let no_boilerplate = BOILERPLATE_RE.replace_all(&no_scripts, " ");
    no_boilerplate.to_string()
}

fn html_to_text(html: &str) -> String {
    let with_breaks = BR_RE.replace_all(html, "\n");
    let with_list_markers = LI_OPEN_RE.replace_all(&with_breaks, "\n- ");
    let with_block_breaks = BLOCK_BREAK_RE.replace_all(&with_list_markers, "\n\n");
    let no_tags = TAG_RE.replace_all(&with_block_breaks, " ");
    let decoded = decode_html_entities(&no_tags);
    normalize_extracted_text(&decoded)
}

fn normalize_whitespace(text: &str) -> String {
    SPACE_RE.replace_all(text.trim(), " ").to_string()
}

fn normalize_extracted_text(text: &str) -> String {
    let normalized_newlines = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = Vec::new();
    let mut last_blank = false;
    for raw_line in normalized_newlines.lines() {
        let line = normalize_whitespace(raw_line);
        if line.is_empty() {
            if !last_blank {
                lines.push(String::new());
            }
            last_blank = true;
        } else {
            lines.push(line);
            last_blank = false;
        }
    }
    let joined = lines.join("\n");
    MANY_NEWLINES_RE
        .replace_all(joined.trim(), "\n\n")
        .to_string()
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
        .replace("&nbsp;", " ")
        .replace("&ndash;", "-")
        .replace("&mdash;", "--")
        .replace("&hellip;", "...")
}
