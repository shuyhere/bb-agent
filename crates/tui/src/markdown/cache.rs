use std::collections::{HashMap, VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use super::blocks::MarkdownBlockToken;

#[derive(Debug)]
struct LruCache<K, V> {
    map: HashMap<K, V>,
    order: VecDeque<K>,
    capacity: usize,
}

impl<K, V> LruCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        let value = self.map.get(key).cloned()?;
        if let Some(index) = self.order.iter().position(|existing| existing == key) {
            self.order.remove(index);
        }
        self.order.push_back(key.clone());
        Some(value)
    }

    fn put(&mut self, key: K, value: V) {
        if self.map.contains_key(&key) {
            self.map.insert(key.clone(), value);
            if let Some(index) = self.order.iter().position(|existing| existing == &key) {
                self.order.remove(index);
            }
            self.order.push_back(key);
            return;
        }

        if self.map.len() >= self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.map.remove(&oldest);
        }

        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }
}

type MarkdownTokenCache = Mutex<LruCache<u64, Vec<MarkdownBlockToken>>>;
type MarkdownRenderCache = Mutex<LruCache<(u64, u16), Vec<String>>>;

static MARKDOWN_TOKEN_CACHE: LazyLock<MarkdownTokenCache> =
    LazyLock::new(|| Mutex::new(LruCache::new(500)));
static MARKDOWN_RENDER_CACHE: LazyLock<MarkdownRenderCache> =
    LazyLock::new(|| Mutex::new(LruCache::new(500)));

pub(super) fn text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn get_cached_tokens(key: u64) -> Option<Vec<MarkdownBlockToken>> {
    MARKDOWN_TOKEN_CACHE
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(&key))
}

pub(super) fn put_cached_tokens(key: u64, tokens: Vec<MarkdownBlockToken>) {
    if let Ok(mut cache) = MARKDOWN_TOKEN_CACHE.lock() {
        cache.put(key, tokens);
    }
}

pub(super) fn get_cached_render(key: (u64, u16)) -> Option<Vec<String>> {
    MARKDOWN_RENDER_CACHE
        .lock()
        .ok()
        .and_then(|mut cache| cache.get(&key))
}

pub(super) fn put_cached_render(key: (u64, u16), lines: Vec<String>) {
    if let Ok(mut cache) = MARKDOWN_RENDER_CACHE.lock() {
        cache.put(key, lines);
    }
}
