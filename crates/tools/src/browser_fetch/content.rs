use regex::Regex;
use std::{sync::LazyLock, time::Duration};

use crate::web::create_web_client;

static CANONICAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<link\b[^>]*rel=["'][^"']*canonical[^"']*["'][^>]*href=["']([^"']+)["']"#)
        .expect("valid canonical regex")
});
static OG_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<meta\b[^>]*property=["']og:url["'][^>]*content=["']([^"']+)["']"#)
        .expect("valid og:url regex")
});

pub(super) async fn resolve_final_url(input_url: &str, dom: &str) -> String {
    if let Some(url) = extract_canonical_like_url(dom) {
        return url;
    }

    let Ok(client) = create_web_client("browser_fetch", Duration::from_secs(10), 10) else {
        return input_url.to_string();
    };

    if let Ok(response) = client.head(input_url).send().await {
        return response.url().to_string();
    }
    if let Ok(response) = client.get(input_url).send().await {
        return response.url().to_string();
    }

    input_url.to_string()
}

pub(super) fn extract_canonical_like_url(dom: &str) -> Option<String> {
    CANONICAL_RE
        .captures(dom)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        .or_else(|| {
            OG_URL_RE
                .captures(dom)
                .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        })
}

pub(super) fn is_browser_protection_page(html: &str) -> bool {
    let lower = html.to_lowercase();
    (lower.contains("captcha")
        || lower.contains("are you human")
        || lower.contains("access denied")
        || lower.contains("verify you are")
        || lower.contains("security checkpoint")
        || lower.contains("unauthorized")
        || lower.contains("please enable javascript")
        || lower.contains("challenge-form"))
        && !lower.contains("<article")
        && !lower.contains("<main")
}
