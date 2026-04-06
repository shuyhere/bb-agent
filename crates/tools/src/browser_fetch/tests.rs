use super::*;
use std::path::Path;

#[test]
fn browser_args_include_dump_dom_and_url() {
    let args = build_browser_args(
        Path::new("/usr/bin/google-chrome"),
        "https://example.com",
        Path::new("/tmp/profile"),
        12.0,
    );
    assert!(args.iter().any(|arg| arg == "--dump-dom"));
    assert!(args.iter().any(|arg| arg == "https://example.com"));
    assert!(
        args.iter()
            .any(|arg| arg.starts_with("--user-data-dir=/tmp/profile"))
    );
}

#[test]
fn canonical_or_og_url_is_extracted() {
    let dom =
        r#"<html><head><link rel="canonical" href="https://example.com/final" /></head></html>"#;
    assert_eq!(
        extract_canonical_like_url(dom).as_deref(),
        Some("https://example.com/final")
    );
}

#[test]
fn protection_page_is_detected() {
    let html =
        r#"<html><body>Unauthorized. Please enable JavaScript. challenge-form</body></html>"#;
    assert!(is_browser_protection_page(html));
}
