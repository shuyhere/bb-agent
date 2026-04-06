use super::*;

static SEARCH_CACHE: LazyLock<Mutex<HashMap<String, CachedSearchEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(super) fn build_cache_key(input: &WebSearchInput) -> String {
    let allowed = input
        .allowed_domains
        .as_ref()
        .map(|domains| {
            let mut domains: Vec<String> = domains
                .iter()
                .map(|domain| normalize_domain(domain))
                .filter(|domain| !domain.is_empty())
                .collect();
            domains.sort();
            domains.join(",")
        })
        .unwrap_or_default();
    let blocked = input
        .blocked_domains
        .as_ref()
        .map(|domains| {
            let mut domains: Vec<String> = domains
                .iter()
                .map(|domain| normalize_domain(domain))
                .filter(|domain| !domain.is_empty())
                .collect();
            domains.sort();
            domains.join(",")
        })
        .unwrap_or_default();
    format!(
        "q={}|allow={}|block={}",
        input.query.trim().to_lowercase(),
        allowed,
        blocked
    )
}

pub(super) fn read_cached_search(cache_key: &str) -> Option<CachedSearchValue> {
    let mut cache = SEARCH_CACHE.lock().ok()?;
    let now = Instant::now();
    cache.retain(|_, entry| entry.expires_at > now);
    cache.get(cache_key).map(|entry| entry.value.clone())
}

pub(super) fn write_cached_search(cache_key: String, value: CachedSearchValue) {
    let Ok(mut cache) = SEARCH_CACHE.lock() else {
        return;
    };
    cache.insert(
        cache_key,
        CachedSearchEntry {
            value,
            expires_at: Instant::now() + Duration::from_secs(CACHE_TTL_SECONDS),
        },
    );
}
