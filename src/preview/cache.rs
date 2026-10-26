//! Bounded, weighted LRU. Entry-capped at 256 and byte-capped so a run of
//! decoded images cannot grow without bound. Clones are cheap for images
//! because their pixels are shared through `Arc<[u8]>`.

use std::collections::VecDeque;
use std::path::PathBuf;

use crate::preview::Preview;

pub const CACHE_ENTRIES: usize = 256;
pub const CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct CacheKey {
    pub path: PathBuf,
    pub len: u64,
    pub mtime_nanos: u128,
    pub is_dir: bool,
}

struct Entry {
    key: CacheKey,
    preview: Preview,
    weight: usize,
}

/// Most recently used entries live at the back of the deque.
pub struct PreviewCache {
    entries: VecDeque<Entry>,
    bytes: usize,
}

impl PreviewCache {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(CACHE_ENTRIES),
            bytes: 0,
        }
    }

    pub fn get(&mut self, key: &CacheKey) -> Option<Preview> {
        let index = self.entries.iter().position(|e| &e.key == key)?;
        let entry = self.entries.remove(index)?;
        let preview = entry.preview.clone();
        self.entries.push_back(entry);
        Some(preview)
    }

    pub fn insert(&mut self, key: CacheKey, preview: Preview) {
        let weight = weight_of(&preview).saturating_add(key.path.as_os_str().len());
        if weight > CACHE_BYTES {
            // A single preview larger than the whole budget is not cached;
            // returning it still works, and the next request rebuilds it.
            return;
        }
        if let Some(index) = self.entries.iter().position(|e| e.key == key)
            && let Some(old) = self.entries.remove(index)
        {
            self.bytes = self.bytes.saturating_sub(old.weight);
        }
        while self.entries.len() >= CACHE_ENTRIES || self.bytes.saturating_add(weight) > CACHE_BYTES
        {
            match self.entries.pop_front() {
                Some(old) => self.bytes = self.bytes.saturating_sub(old.weight),
                None => break,
            }
        }
        self.entries.push_back(Entry {
            key,
            preview,
            weight,
        });
        self.bytes = self.bytes.saturating_add(weight);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
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

    #[test]
    fn byte_budget_evicts_older_large_previews() {
        let mut cache = PreviewCache::new();
        // Four MiB each: twenty entries cannot fit in a 64 MiB budget even
        // though the entry count is well below the 256 cap.
        let pixels: Arc<[u8]> = vec![7u8; 4 * 1024 * 1024].into();
        for i in 0..20 {
            cache.insert(image_key(i), large_image(&pixels));
            assert!(cache.bytes <= CACHE_BYTES);
        }
        assert!(cache.len() < 20, "byte cap did not evict");
        assert_eq!(cache.get(&image_key(19)).map(|p| p.kind()), Some("IMAGE"));
        assert!(
            cache.get(&image_key(0)).is_none(),
            "oldest entry was evicted"
        );
    }
}
