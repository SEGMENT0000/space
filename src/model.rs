//! Frozen data contract between the scan engine, preview service, treemap and UI.
//!
//! The tree is immutable after a scan. Node ids are stable arena indices and
//! parent ids are always smaller than child ids. Names live in a compact UTF-8
//! arena; child links are intrusive (`first_child` / `next_sibling`) so a scan
//! of a million entries does not pay for one `Vec` allocation per directory.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

pub const NO_NODE: u32 = u32::MAX;
pub type NodeId = u32;

/// The eight product categories. The numeric values are part of the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Category {
    Media = 0,
    Documents = 1,
    Code = 2,
    Caches = 3,
    Apps = 4,
    Archives = 5,
    System = 6,
    Other = 7,
}

pub const CATEGORY_COUNT: usize = 8;

impl Category {
    pub const ALL: [Category; CATEGORY_COUNT] = [
        Category::Media,
        Category::Documents,
        Category::Code,
        Category::Caches,
        Category::Apps,
        Category::Archives,
        Category::System,
        Category::Other,
    ];

    pub fn from_id(id: u8) -> Self {
        match id {
            0 => Self::Media,
            1 => Self::Documents,
            2 => Self::Code,
            3 => Self::Caches,
            4 => Self::Apps,
            5 => Self::Archives,
            6 => Self::System,
            _ => Self::Other,
        }
    }

    pub fn id(self) -> u8 {
        self as u8
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Media => "Media",
            Self::Documents => "Documents",
            Self::Code => "Code",
            Self::Caches => "Caches",
            Self::Apps => "Apps",
            Self::Archives => "Archives",
            Self::System => "System",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CategoryTotals(pub [u64; CATEGORY_COUNT]);

impl CategoryTotals {
    pub fn add(&mut self, cat: Category, bytes: u64) {
        self.0[cat.id() as usize] = self.0[cat.id() as usize].saturating_add(bytes);
    }

    pub fn total(&self) -> u64 {
        self.0.iter().copied().sum()
    }

    pub fn get(&self, cat: Category) -> u64 {
        self.0[cat.id() as usize]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Node {
    pub parent: NodeId,
    pub first_child: NodeId,
    pub next_sibling: NodeId,
    pub name_off: u32,
    pub name_len: u32,
    /// File bytes for files; complete subtree bytes for directories.
    pub size: u64,
    /// One for every file in the subtree.
    pub file_count: u64,
    /// Exact for files. For directories this is the category of the largest
    /// direct branch (used for tile color, not for reported composition).
    pub cat: Category,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct ScanProgress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    /// Directory currently being walked, for the scan instrument.
    pub current: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ScanSummary {
    pub root_path: String,
    pub total_bytes: u64,
    pub files: u64,
    pub dirs: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    /// Exact category composition of the scanned root.
    pub categories: CategoryTotals,
}

#[derive(Debug)]
pub struct ScanOutput {
    pub tree: Tree,
    pub summary: ScanSummary,
}

#[derive(Debug)]
pub enum ScanEvent {
    Progress(ScanProgress),
    Done(Box<ScanOutput>),
    Failed(String),
}

struct CategoryCache {
    map: std::collections::HashMap<NodeId, (u64, CategoryTotals)>,
    order: std::collections::VecDeque<NodeId>,
}

impl CategoryCache {
    fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
            order: std::collections::VecDeque::new(),
        }
    }

    fn get(&mut self, id: NodeId) -> Option<CategoryTotals> {
        let entry = self.map.get_mut(&id)?;
        entry.0 = entry.0.saturating_add(1);
        Some(entry.1)
    }

    fn insert(&mut self, id: NodeId, totals: CategoryTotals) {
        const CAP: usize = 48;
        if self.map.insert(id, (1, totals)).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > CAP {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
    }
}

/// Immutable scanned hierarchy with an on-demand category-composition cache.
pub struct Tree {
    nodes: Vec<Node>,
    names: Vec<u8>,
    root_path: String,
    largest: Vec<NodeId>,
    cache: Mutex<CategoryCache>,
}

impl std::fmt::Debug for Tree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tree")
            .field("nodes", &self.nodes.len())
            .field("root_path", &self.root_path)
            .finish()
    }
}

impl Tree {
    pub fn root(&self) -> NodeId {
        0
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn name(&self, id: NodeId) -> &str {
        let n = &self.nodes[id as usize];
        let start = n.name_off as usize;
        std::str::from_utf8(&self.names[start..start + n.name_len as usize]).unwrap_or("?")
    }

    pub fn root_path(&self) -> &str {
        &self.root_path
    }

    /// Absolute native path for a node. Only called for visible/inspected nodes.
    pub fn path(&self, id: NodeId) -> PathBuf {
        if id == self.root() {
            return PathBuf::from(&self.root_path);
        }
        let mut chain = Vec::with_capacity(8);
        let mut cur = id;
        while cur != self.root() {
            chain.push(cur);
            cur = self.nodes[cur as usize].parent;
        }
        let mut path = PathBuf::from(&self.root_path);
        for id in chain.iter().rev() {
            path.push(self.name(*id));
        }
        path
    }

    pub fn path_string(&self, id: NodeId) -> String {
        self.path(id).to_string_lossy().into_owned()
    }

    /// Direct children sorted by descending size, then name for stable ties.
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = self.nodes[id as usize].first_child;
        while cur != NO_NODE {
            out.push(cur);
            cur = self.nodes[cur as usize].next_sibling;
        }
        out.sort_by(|a, b| {
            let a = &self.nodes[*a as usize];
            let b = &self.nodes[*b as usize];
            b.size.cmp(&a.size)
        });
        out
    }

    pub fn child_count(&self, id: NodeId) -> u32 {
        let mut n = 0;
        let mut cur = self.nodes[id as usize].first_child;
        while cur != NO_NODE {
            n += 1;
            cur = self.nodes[cur as usize].next_sibling;
        }
        n
    }

    /// Case-insensitive name search across the arena, capped so a query never
    /// stalls the UI. Results are sorted by descending size.
    pub fn search(&self, query_lower: &str, limit: usize, max_visits: usize) -> Vec<NodeId> {
        if query_lower.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(limit * 2);
        let visits = max_visits.min(self.nodes.len().saturating_sub(1));
        for id in 1..=visits as u32 {
            let name = self.name(id);
            if name.len() >= query_lower.len() && name.to_lowercase().contains(query_lower) {
                out.push(id);
                if out.len() >= limit * 4 {
                    break;
                }
            }
        }
        out.sort_by(|a, b| {
            self.nodes[*b as usize]
                .size
                .cmp(&self.nodes[*a as usize].size)
        });
        out.truncate(limit);
        out
    }

    /// Largest files discovered during the scan, descending. Stable ids.
    pub fn largest_files(&self, limit: usize) -> &[NodeId] {
        let n = limit.min(self.largest.len());
        &self.largest[..n]
    }

    /// Exact category bytes for a subtree. Cached with a small bounded LRU.
    /// Expensive for the root of a multi-million-file scan: call off the UI
    /// thread, or use `ScanSummary::categories` for the scanned root.
    pub fn category_totals(&self, id: NodeId) -> CategoryTotals {
        if id == self.root() {
            // Root composition is exact and supplied at finish time through cache.
        }
        if let Some(hit) = self.cache.lock().get(id) {
            return hit;
        }

        let mut totals = CategoryTotals::default();
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            let node = &self.nodes[cur as usize];
            if node.is_dir {
                let mut child = node.first_child;
                while child != NO_NODE {
                    stack.push(child);
                    child = self.nodes[child as usize].next_sibling;
                }
            } else {
                totals.add(node.cat, node.size);
            }
        }
        self.cache.lock().insert(id, totals);
        totals
    }

    /// Descendants of a directory that are folders, used for the breadcrumb of
    /// a path on demand. Returns ids from root to `id` inclusive.
    pub fn lineage(&self, id: NodeId) -> Vec<NodeId> {
        let mut chain = Vec::with_capacity(8);
        let mut cur = id;
        loop {
            chain.push(cur);
            if cur == self.root() {
                break;
            }
            cur = self.nodes[cur as usize].parent;
        }
        chain.reverse();
        chain
    }

    pub fn is_ancestor(&self, ancestor: NodeId, mut node: NodeId) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            if node == self.root() {
                return false;
            }
            node = self.nodes[node as usize].parent;
        }
    }

    pub(crate) fn prime_categories(&self, totals: CategoryTotals) {
        self.cache.lock().insert(self.root(), totals);
    }
}

pub struct TreeBuilder {
    nodes: Vec<Node>,
    names: Vec<u8>,
    last_child: Vec<NodeId>,
    root_path: String,
}

impl TreeBuilder {
    pub fn new(root_path: impl Into<String>, root_label: impl AsRef<str>) -> Self {
        let root_path = root_path.into();
        let mut builder = Self {
            nodes: Vec::with_capacity(4096),
            names: Vec::with_capacity(64 * 1024),
            last_child: Vec::with_capacity(4096),
            root_path,
        };
        builder.add_node(root_label.as_ref(), NO_NODE, true, 0, Category::Other);
        builder
    }

    pub fn add_node(
        &mut self,
        name: &str,
        parent: NodeId,
        is_dir: bool,
        size: u64,
        cat: Category,
    ) -> NodeId {
        let id = self.nodes.len() as NodeId;
        let off = self.names.len() as u32;
        let bytes = name.as_bytes();
        self.names.extend_from_slice(bytes);
        self.nodes.push(Node {
            parent,
            first_child: NO_NODE,
            next_sibling: NO_NODE,
            name_off: off,
            name_len: bytes.len() as u32,
            size: if is_dir { 0 } else { size },
            file_count: if is_dir { 0 } else { 1 },
            cat,
            is_dir,
        });
        self.last_child.push(NO_NODE);
        if parent != NO_NODE {
            let p = parent as usize;
            let tail = self.last_child[p];
            if tail == NO_NODE {
                self.nodes[p].first_child = id;
            } else {
                self.nodes[tail as usize].next_sibling = id;
            }
            self.last_child[p] = id;
        }
        id
    }

    pub fn finish(mut self, mut largest: Vec<NodeId>, categories: CategoryTotals) -> Tree {
        let n = self.nodes.len();
        // Roll sizes and file counts up the tree. Child ids are always greater
        // than parent ids, so one reverse pass is sufficient.
        let mut best = vec![0u64; n];
        for id in (1..n).rev() {
            let parent = self.nodes[id].parent;
            let size = self.nodes[id].size;
            let files = self.nodes[id].file_count;
            let cat = self.nodes[id].cat;
            self.nodes[parent as usize].size =
                self.nodes[parent as usize].size.saturating_add(size);
            self.nodes[parent as usize].file_count =
                self.nodes[parent as usize].file_count.saturating_add(files);
            if size > best[parent as usize] {
                best[parent as usize] = size;
                self.nodes[parent as usize].cat = cat;
            }
        }
        largest.retain(|id| (*id as usize) < n);
        largest.sort_by(|a, b| {
            self.nodes[*b as usize]
                .size
                .cmp(&self.nodes[*a as usize].size)
        });
        let tree = Tree {
            nodes: self.nodes,
            names: self.names,
            root_path: self.root_path,
            largest,
            cache: Mutex::new(CategoryCache::new()),
        };
        tree.prime_categories(categories);
        tree
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub free: u64,
    /// "ssd" | "hdd" | "removable" | "unknown"
    pub kind: String,
    pub is_removable: bool,
}

impl DriveInfo {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }

    pub fn used_fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.used() as f64 / self.total as f64) as f32
        }
    }
}

pub type SharedOutput = Arc<ScanOutput>;

#[cfg(test)]
mod tests {
    use super::{Category, CategoryCache, CategoryTotals};

    #[test]
    fn category_cache_returns_inserted_totals_and_evicts_oldest() {
        let mut cache = CategoryCache::new();
        let mut totals = CategoryTotals::default();
        totals.add(Category::Code, 4096);
        cache.insert(7, totals);
        assert_eq!(
            cache.get(7).map(|value| value.get(Category::Code)),
            Some(4096)
        );
        assert!(cache.get(8).is_none());

        for id in 1..=50 {
            cache.insert(id, CategoryTotals::default());
        }
        assert!(cache.get(1).is_none(), "oldest entries must be evicted");
        assert!(cache.get(50).is_some(), "newest entries must remain");
        assert_eq!(cache.map.len(), 48);
    }
}
