use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use tiny_skia::Pixmap;

#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub bytes: u64,
    pub entries: u64,
}

struct Entry {
    pixmap: std::sync::Arc<Pixmap>,
    bytes: usize,
}

struct Inner {
    entries: HashMap<String, Entry>,
    order: VecDeque<String>,
    bytes: usize,
    hits: u64,
    misses: u64,
}

impl Inner {
    fn touch(&mut self, key: &str) {
        self.order.retain(|existing| existing != key);
        self.order.push_back(key.to_owned());
    }

    fn remove(&mut self, key: &str) {
        let Some(entry) = self.entries.remove(key) else {
            return;
        };
        self.bytes = self.bytes.saturating_sub(entry.bytes);
        self.order.retain(|existing| existing != key);
    }

    fn evict(&mut self, max_bytes: usize) {
        while self.bytes > max_bytes {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(entry.bytes);
            }
        }
    }
}

pub struct PixmapCache {
    inner: Mutex<Inner>,
    max_bytes: usize,
}

impl PixmapCache {
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                entries: HashMap::new(),
                order: VecDeque::new(),
                bytes: 0,
                hits: 0,
                misses: 0,
            }),
            max_bytes,
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<std::sync::Arc<Pixmap>> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = inner.entries.get(key) {
            let pixmap = std::sync::Arc::clone(&entry.pixmap);
            inner.hits += 1;
            inner.touch(key);
            return Some(pixmap);
        }
        inner.misses += 1;
        None
    }

    pub fn insert(&self, key: String, pixmap: Pixmap) {
        if self.max_bytes == 0 {
            return;
        }
        let bytes = pixmap.width() as usize * pixmap.height() as usize * 4;
        if bytes > self.max_bytes {
            return;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.remove(&key);
        inner.bytes += bytes;
        inner.order.push_back(key.clone());
        inner.entries.insert(
            key,
            Entry {
                pixmap: std::sync::Arc::new(pixmap),
                bytes,
            },
        );
        inner.evict(self.max_bytes);
    }

    pub fn clear(&self) -> u64 {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let bytes = inner.bytes as u64;
        inner.entries.clear();
        inner.order.clear();
        inner.bytes = 0;
        bytes
    }

    #[must_use]
    pub fn stats(&self) -> CacheStats {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        CacheStats {
            hits: inner.hits,
            misses: inner.misses,
            bytes: inner.bytes as u64,
            entries: inner.entries.len() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::{Color, Pixmap};

    fn pixmap() -> Pixmap {
        let mut pixmap = Pixmap::new(4, 4).unwrap();
        pixmap.fill(Color::BLACK);
        pixmap
    }

    #[test]
    fn caches_and_evicts() {
        let cache = PixmapCache::new(64);
        cache.insert("a".into(), pixmap());
        assert!(cache.get("a").is_some());
        cache.insert("b".into(), pixmap());
        assert!(cache.get("a").is_none());
        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn ignores_oversized_pixmaps() {
        let cache = PixmapCache::new(16);
        cache.insert("a".into(), pixmap());
        assert!(cache.get("a").is_none());
    }
}
