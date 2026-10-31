//! Bounded, weighted LRU. Entry-capped at 256 and byte-capped so a run of
//! decoded images cannot grow without bound. Clones are cheap for images
//! because their pixels are shared through `Arc<[u8]>`.
//!
//! The same generic cache backs both the inspector preview cache and the
//! on-tile art cache; each wrapper owns its caps and weight function.

use std::collections::VecDeque;
use std::path::PathBuf;

use crate::preview::Preview;

pub const CACHE_ENTRIES: usize = 256;
pub const CACHE_BYTES: usize = 64 * 1024 * 1024;

/// Generic count- and byte-bounded LRU. Most recently used entries live at
/// the back of the deque.
pub(crate) struct Lru<K, V> {
    entries: VecDeque<(K, V, usize)>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
    weight: fn(&V) -> usize,
}

impl<K: PartialEq, V> Lru<K, V> {
    pub fn new(max_entries: usize, max_bytes: usize, weight: fn(&V) -> usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries.min(64)),
            bytes: 0,
            max_entries: max_entries.max(1),
            max_bytes: max_bytes.max(1),
            weight,
        }
    }

    pub fn get(&mut self, key: &K) -> Option<V>
    where
        V: Clone,
        K: Clone,
    {
        let index = self.entries.iter().position(|(k, _, _)| k == key)?;
        let (_, value, weight) = self.entries.remove(index)?;
        self.bytes = self.bytes.saturating_sub(weight);
        let out = value.clone();
        // Re-insert through the normal path so the weight accounting stays in
        // one place; a hit cannot exceed the budget because it already fit.
        self.insert(key.clone(), value);
        Some(out)
    }

    pub fn insert(&mut self, key: K, value: V) {
        let weight = (self.weight)(&value);
        if weight > self.max_bytes {
            // A single entry larger than the whole budget is not cached;
            // returning it still works, and the next request rebuilds it.
            return;
        }
        if let Some(index) = self.entries.iter().position(|(k, _, _)| *k == key)
            && let Some((_, _, old_weight)) = self.entries.remove(index)
        {
            self.bytes = self.bytes.saturating_sub(old_weight);
        }
        while self.entries.len() >= self.max_entries
            || self.bytes.saturating_add(weight) > self.max_bytes
        {
            match self.entries.pop_front() {
                Some((_, _, old_weight)) => self.bytes = self.bytes.saturating_sub(old_weight),
                None => break,
            }
        }
        self.entries.push_back((key, value, weight));
        self.bytes = self.bytes.saturating_add(weight);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Current byte total. Reachable from unit tests, which assert the byte
    /// cap; the read-only build does not need it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CacheKey {
    pub path: PathBuf,
    pub len: u64,
    pub mtime_nanos: u128,
    pub is_dir: bool,
}

/// Bounded preview cache for the inspector.
pub struct PreviewCache {
    inner: Lru<CacheKey, Preview>,
}

impl PreviewCache {
    pub fn new() -> Self {
        Self {
            inner: Lru::new(CACHE_ENTRIES, CACHE_BYTES, weight_of),
        }
    }

    pub fn get(&mut self, key: &CacheKey) -> Option<Preview> {
        self.inner.get(key)
    }

    pub fn insert(&mut self, key: CacheKey, preview: Preview) {
        self.inner.insert(key, preview);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

fn weight_of(preview: &Preview) -> usize {
    let mut n = 128usize;
    match preview {
        Preview::Text(t) => {
            n = n.saturating_add(64);
            for line in &t.lines {
                n = n.saturating_add(line.len()).saturating_add(24);
            }
            if let Some(lang) = &t.language {
                n = n.saturating_add(lang.len());
            }
            if let Some(enc) = &t.encoding {
                n = n.saturating_add(enc.len());
            }
        }
        Preview::Hex(h) => {
            for row in &h.rows {
                n = n
                    .saturating_add(row.hex.len())
                    .saturating_add(row.ascii.len())
                    .saturating_add(32);
            }
        }
        Preview::Image(i) => {
            n = n
                .saturating_add(i.rgba.len())
                .saturating_add(i.mime.len())
                .saturating_add(64);
        }
        Preview::List(l) => {
            n = n.saturating_add(l.kind.len());
            for e in &l.entries {
                n = n.saturating_add(e.name.len()).saturating_add(64);
                if let Some(m) = &e.method {
                    n = n.saturating_add(m.len());
                }
            }
        }
        Preview::Meta(m) => {
            for p in &m.pairs {
                n = n
                    .saturating_add(p.key.len())
                    .saturating_add(p.value.len())
                    .saturating_add(32);
            }
            if let Some(note) = &m.note {
                n = n.saturating_add(note.len());
            }
        }
        Preview::Db(d) => {
            for t in &d.tables {
                n = n.saturating_add(t.name.len()).saturating_add(64);
                if let Some(cols) = &t.columns {
                    for c in cols {
                        n = n.saturating_add(c.len()).saturating_add(16);
                    }
                }
                if let Some(rows) = &t.rows {
                    for row in rows {
                        for cell in row {
                            n = n.saturating_add(cell.len()).saturating_add(16);
                        }
                    }
                }
            }
            if let Some(note) = &d.note {
                n = n.saturating_add(note.len());
            }
        }
        Preview::Media(m) => {
            n = n
                .saturating_add(m.kind.len())
                .saturating_add(m.mime.len())
                .saturating_add(64);
            for p in &m.pairs {
                n = n
                    .saturating_add(p.key.len())
                    .saturating_add(p.value.len())
                    .saturating_add(32);
            }
            if let Some(t) = &m.thumbnail {
                n = n.saturating_add(t.rgba.len()).saturating_add(64);
            }
            if let Some(note) = &m.note {
                n = n.saturating_add(note.len());
            }
        }
        Preview::Unavailable { reason } => {
            n = n.saturating_add(reason.len());
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::ImagePreview;
    use std::sync::Arc;

    fn image_key(i: usize) -> CacheKey {
        CacheKey {
            path: PathBuf::from(format!("image-{i}.png")),
            len: i as u64,
            mtime_nanos: i as u128,
            is_dir: false,
        }
    }

    fn large_image(pixels: &Arc<[u8]>) -> Preview {
        Preview::Image(ImagePreview {
            width: 1,
            height: 1,
            rgba: pixels.clone(),
            mime: "image/png".to_string(),
            source_width: 1,
            source_height: 1,
        })
    }

    fn byte_weight(preview: &Preview) -> usize {
        match preview {
            Preview::Image(i) => i.rgba.len(),
            _ => 0,
        }
    }

    #[test]
    fn byte_budget_evicts_older_large_previews() {
        let mut cache = PreviewCache::new();
        // Four MiB each: twenty entries cannot fit in a 64 MiB budget even
        // though the entry count is well below the 256 cap.
        let pixels: Arc<[u8]> = vec![7u8; 4 * 1024 * 1024].into();
        for i in 0..20 {
            cache.insert(image_key(i), large_image(&pixels));
            assert!(cache.inner.bytes() <= CACHE_BYTES);
        }
        assert!(cache.len() < 20, "byte cap did not evict");
        assert_eq!(cache.get(&image_key(19)).map(|p| p.kind()), Some("IMAGE"));
        assert!(
            cache.get(&image_key(0)).is_none(),
            "oldest entry was evicted"
        );
    }

    #[test]
    fn entry_cap_evicts_oldest_before_the_byte_cap() {
        let mut cache = Lru::new(4, 1024, byte_weight);
        for i in 0..6 {
            cache.insert(
                i,
                Preview::Unavailable {
                    reason: format!("r{i}"),
                },
            );
            assert!(cache.len() <= 4);
            assert!(cache.bytes() <= 1024);
        }
        assert_eq!(cache.len(), 4);
        assert!(cache.get(&0).is_none());
        assert!(cache.get(&5).is_some());
    }

    #[test]
    fn byte_cap_evicts_lru_entries_and_touch_refreshes() {
        let mut cache = Lru::new(64, 3 * 1024, byte_weight);
        let pixels: Arc<[u8]> = vec![1u8; 1024].into();
        for i in 0..4 {
            cache.insert(i, large_image(&pixels));
        }
        assert!(cache.bytes() <= 3 * 1024);
        assert_eq!(cache.len(), 3, "byte cap must evict the oldest entry");
        assert!(cache.get(&0).is_none());
        // Touching entry 1 makes entry 2 the oldest.
        assert!(cache.get(&1).is_some());
        cache.insert(4, large_image(&pixels));
        assert!(
            cache.get(&2).is_none(),
            "untouched entry should evict first"
        );
        assert!(cache.get(&1).is_some(), "recently used entry must survive");
        assert!(cache.bytes() <= 3 * 1024);
    }

    #[test]
    fn an_entry_larger_than_the_budget_is_never_cached() {
        let mut cache = Lru::new(8, 512, byte_weight);
        let pixels: Arc<[u8]> = vec![0u8; 1024].into();
        cache.insert(0, large_image(&pixels));
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.bytes(), 0);
    }
}
