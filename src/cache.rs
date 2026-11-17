use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use tiny_skia::Pixmap;

#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub bytes: u64,
    pub entries: u64,
}

struct Entry {
    pixmap: Arc<Pixmap>,
    bytes: usize,
    stamp: u64,
}

struct Inner {
    entries: HashMap<String, Entry>,
    order: VecDeque<(u64, String)>,
    bytes: usize,
    hits: u64,
    misses: u64,
    clock: u64,
}

impl Inner {
    fn next_stamp(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    fn touch(&mut self, key: &str) {
        let stamp = self.next_stamp();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.stamp = stamp;
            self.order.push_back((stamp, key.to_owned()));
        }
        self.prune_order();
    }

    fn remove(&mut self, key: &str) {
        if let Some(entry) = self.entries.remove(key) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
        }
    }

    fn prune_order(&mut self) {
        let limit = self.entries.len().saturating_mul(2).max(8);
        if self.order.len() <= limit {
            return;
        }
        let entries = &self.entries;
        self.order
            .retain(|(stamp, key)| entries.get(key).is_some_and(|entry| entry.stamp == *stamp));
    }

    fn evict(&mut self, max_bytes: usize) {
        while self.bytes > max_bytes {
            let Some((stamp, key)) = self.order.pop_front() else {
                break;
            };
            let is_current = self
                .entries
                .get(&key)
                .is_some_and(|entry| entry.stamp == stamp);
            if is_current && let Some(entry) = self.entries.remove(&key) {
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
                clock: 0,
            }),
            max_bytes,
        }
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<Arc<Pixmap>> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let pixmap = inner
            .entries
            .get(key)
            .map(|entry| Arc::clone(&entry.pixmap));
        if let Some(pixmap) = pixmap {
            inner.hits += 1;
            inner.touch(key);
            return Some(pixmap);
        }
        inner.misses += 1;
        None
    }

    #[cfg(test)]
    pub fn insert(&self, key: String, pixmap: Pixmap) {
        self.insert_arc(key, Arc::new(pixmap));
    }

    pub fn insert_arc(&self, key: String, pixmap: Arc<Pixmap>) {
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
        let stamp = inner.next_stamp();
        inner.order.push_back((stamp, key.clone()));
        inner.entries.insert(
            key,
            Entry {
                pixmap,
                bytes,
                stamp,
            },
        );
        inner.prune_order();
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
        inner.clock = 0;
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

    #[test]
    fn keeps_recently_used_entries() {
        let cache = PixmapCache::new(128);
        cache.insert("a".into(), pixmap());
        cache.insert("b".into(), pixmap());
        assert!(cache.get("a").is_some());
        cache.insert("c".into(), pixmap());
        assert!(cache.get("a").is_some());
        assert!(cache.get("b").is_none());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn replaces_existing_entries_without_double_counting() {
        let cache = PixmapCache::new(128);
        cache.insert("a".into(), pixmap());
        cache.insert("a".into(), pixmap());
        let stats = cache.stats();
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.bytes, 64);
    }

    #[test]
    fn zero_budget_disables_storage() {
        let cache = PixmapCache::new(0);
        cache.insert("a".into(), pixmap());
        assert!(cache.get("a").is_none());
    }
}
