//! The scanning engine.
//!
//! `Engine` owns every piece of scan-related state: the arena of [`Node`]s built
//! during a walk, the aggregates computed from it, the user-facing scan state
//! and the caches used to answer tree queries without recomputation.
//!
//! A scan runs on its own thread(s) while the Tauri main thread keeps answering
//! queries and emitting `scan://progress` events.
//!
//! On Windows the walk is a pool of workers running `FindFirstFileExW` directly
//! (large-fetch mode, no per-file stat, reparse points skipped) fed from one
//! shared directory queue. Entries accumulate in a worker-local staging arena
//! (UTF-16 names copied straight from the find data), and a whole batch — the
//! files AND the subdirectories just discovered — reaches the shared store in
//! ONE lock acquisition; those subdirectories are enqueued right after it, so
//! the other workers fan out while the listing is still running. Names are
//! materialized into `Node.name` once at finalize, so the per-file hot loop
//! allocates nothing: no `OsString`, no lossy `String`, no lowercased
//! extension, no shared lock.
//!
//! On other platforms `jwalk` (parallel via rayon, `follow_links(false)` so
//! symlinks are never followed and never double counted) drives the same
//! staging buffers through per-listing bulk appends, with the same zero-alloc
//! classification fast path.
//!
//! PERF-plan references (owner recon): the staged-arena name path replaces the
//! per-entry `OsString`→`String` double allocation; worker-local top lists
//! replace the shared top-files heap; bulk appends replace DashMap churn; the
//! queue uses batched pops + gated `notify_one`; workers run at below-normal
//! priority and are panic-safe.

use std::cmp::Ordering;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use std::path::PathBuf;
#[cfg(not(windows))]
use std::path::Path;
#[cfg(not(windows))]
use dashmap::DashMap;
#[cfg(not(windows))]
use jwalk::{DirEntry, WalkDir};
#[cfg(windows)]
use parking_lot::Condvar;
use parking_lot::Mutex as ParkingMutex;
use rayon::prelude::*;
use tauri::Emitter;

#[cfg(windows)]
use std::collections::VecDeque;
use std::sync::atomic::AtomicU32;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
    FIND_FIRST_EX_LARGE_FETCH, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    WIN32_FIND_DATAW,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};

use crate::classify;
use crate::model::{
    self, ChildEntry, ChildrenResponse, DonePayload, ErrorPayload, LargestFile, Node, ProgressPayload,
    ScanStateDto,
};

/// Interval at which the progress reporter emits `scan://progress`.
const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(120);

/// Maximum number of entries kept in the top-files list (per worker and in
/// the merged global list — the same bound argument as the old heap: a file
/// can only be the global 1001st-or-worse if 1000 strictly larger files exist).
const TOP_FILES_CAP: usize = 1000;

/// Local top lists are re-bounded (sort + truncate) once they grow past this.
const TOP_FILES_RESIZE: usize = 2 * TOP_FILES_CAP;

/// Maximum number of entries kept in the sorted-children LRU (keyed by
/// node id + sort tag; also serves the depth≥2 sub-entries). Raised from 64:
/// entries are `Arc<[u32]>`, so each one costs one refcount on hit.
const SORTED_CACHE_CAP: usize = 256;

/// Maximum number of sub-entries attached to a directory node.
const SUB_ENTRIES: u32 = 8;

/// Limit used by `get_children` when the caller asks for "all" children.
const DEFAULT_CHILDREN_LIMIT: u32 = 100_000;

/// How many queue items a worker drains per wake (batched handoff: far fewer
/// lock round-trips and futex sleeps than one item per wake). Large enough
/// that a burst of discovered subdirectories reaches a single worker in one
/// lock round-trip, small enough that the batch is handed back quickly when
/// the tree turns narrow.
#[cfg(windows)]
const POP_BATCH: usize = 32;

/// Staged entries accumulate per worker until this many, then flush in one
/// bulk append. Larger batches amortize the store lock over more directories
/// without delaying directory discovery (ids for subdirs are assigned at
/// discovery and never wait for a flush).
#[cfg(windows)]
const FLUSH_BATCH: usize = 512;

/// Bit mask of [`FLUSH_BATCH`]: the walk hands its staging buffers over (and
/// observes cancellation) exactly every `FLUSH_BATCH` enumerated entries, with
/// no division.
#[cfg(windows)]
const FLUSH_MASK: u32 = (FLUSH_BATCH as u32) - 1;

/// How the engine reports progress to the outside world.
///
/// The Tauri layer plugs in an emitter that forwards every payload to the
/// webview; tests and probes can plug in whatever they need. Keeping this a
/// trait keeps the engine itself free of Tauri types.
pub trait ScanEvents: Send + Sync + 'static {
    /// Emitted every [`PROGRESS_INTERVAL`] while the walk runs.
    fn progress(&self, payload: ProgressPayload);
    /// Emitted once when the walk ended and the data is published.
    fn done(&self, payload: DonePayload);
    /// Emitted once when the scan failed and no data was published.
    fn error(&self, payload: ErrorPayload);
}

/// One (parent, child) id pair waiting to become a children list entry.
type ChildPair = (u32, u32);

/// Shared, lock-guarded arena the walkers append into.
///
/// All vectors are append-only during a walk; `finish_scan` drains them once.
/// Node ids stay dense and topological (`nodes[id]` is the node with id `id`,
/// every node's id > its parent's id) because ids are handed out under the
/// store lock as `nodes.len()`, either per directory-discovery (a directory's
/// id must exist before its listing runs) or in one bulk append per flush.
struct StoreInner {
    nodes: Vec<Node>,
    /// Name bytes of every node so far, keyed by [`StoreInner::spans`]: real
    /// UTF-16 on Windows (straight from `WIN32_FIND_DATAW`), raw OsStr bytes
    /// zero-extended on other platforms. Written once per node, read once at
    /// finalize — the hot loop never builds a `String` per file.
    arena: Vec<u16>,
    /// `(arena offset, len)` per node, in the same order as `nodes`.
    spans: Vec<(u32, u32)>,
    /// (parent id, child id) log — merged into the dense children lists in
    /// one pass at finalize (replaces the old DashMap children accumulation).
    child_pairs: Vec<ChildPair>,
}

impl Default for StoreInner {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            arena: Vec::new(),
            spans: Vec::new(),
            child_pairs: Vec::new(),
        }
    }
}

/// Everything the walkers share.
type SharedStore = Arc<ParkingMutex<StoreInner>>;

/// Per-worker staging buffers (one per worker thread on Windows; one per
/// directory listing on the jwalk path). Nothing in here allocates per file:
/// every vector grows amortized and is drained into the store in bulk.
struct WorkerCtx {
    /// Staged name bytes (local offsets, rebased at flush).
    arena: Vec<u16>,
    /// Staged nodes (`name` stays empty until finalize materializes it).
    nodes: Vec<Node>,
    /// `(local arena offset, len)` per staged node.
    spans: Vec<(u32, u32)>,
    /// `(parent id, local node index)` per staged entry; rebased to node ids
    /// at flush.
    pairs: Vec<ChildPair>,
    /// Local top-file candidates `(size, node id)`. Kept per worker — no
    /// shared heap lock per file; merged once at finalize.
    top: Vec<(u64, u32)>,
    /// Subdirectories discovered so far, staged TOGETHER with their node:
    /// their global id is only assigned when the batch is appended to the
    /// store (one lock per batch instead of one per subdirectory), and they
    /// are enqueued in the same step (drained into the queue under one lock).
    #[cfg(windows)]
    subdirs: Vec<StagedSubdir>,
}

impl Default for WorkerCtx {
    fn default() -> Self {
        Self {
            arena: Vec::new(),
            nodes: Vec::new(),
            spans: Vec::new(),
            pairs: Vec::new(),
            top: Vec::new(),
            #[cfg(windows)]
            subdirs: Vec::new(),
        }
    }
}

/// Everything the engine knows about one finished scan.
#[allow(dead_code)]
struct ScanData {
    root_id: u32,
    root_path: String,
    nodes: Vec<Node>,
    /// CSR index of every node's direct children: `node_id`'s children are
    /// `child_ids[child_offsets[node_id]..child_offsets[node_id + 1]]`.
    /// Built in three O(n) passes over the (parent, child) log at finalize —
    /// two flat arrays instead of one heap `Vec` (plus 24 bytes of `Vec`
    /// header) per directory.
    child_offsets: Vec<u32>,
    child_ids: Vec<u32>,
    /// Per-category subtree totals of every node, indexed by node id, written
    /// by `finish_scan`'s reverse pass. Zeroed for files except their own
    /// bucket (so `get_category_totals(file_id)` still answers, like the old
    /// subtree walk did). Makes every category query O(1) (§C5).
    dir_totals: Vec<[u64; 8]>,
    /// Per-category byte totals of the whole scan (the root's view).
    categories: Vec<u64>,
    /// Ids of the largest files, sorted by size descending.
    top_files: Vec<u32>,
    /// Full paths of `top_files`, materialized ONCE at finalize so
    /// `get_largest_files` never rebuilds ancestor chains per request.
    top_paths: Vec<String>,
    total_bytes: u64,
    total_files: u64,
    total_dirs: u64,
    errors: u64,
    elapsed_ms: u64,
}

/// Direct children of `node_id` in the CSR index: the slice
/// `child_ids[child_offsets[node_id]..child_offsets[node_id + 1]]`, or empty
/// for files and out-of-range ids (the same answer the old
/// `children.get(id).map_or(0, len)` produced). O(1), no allocation.
#[inline]
fn children_of(data: &ScanData, node_id: u32) -> &[u32] {
    let index = node_id as usize;
    let (start, end) = match (
        data.child_offsets.get(index),
        data.child_offsets.get(index + 1),
    ) {
        (Some(&start), Some(&end)) if end >= start => (start as usize, end as usize),
        _ => return &[],
    };
    data.child_ids.get(start..end).unwrap_or(&[])
}

/// Node arena plus the maps needed to answer tree queries.
///
/// `data`, `current`, the counters and the caches are shared with the worker
/// threads, hence the interior mutability.
pub struct Engine {
    /// Result of the last (or currently running) scan.
    ///
    /// Published once by `finish_scan` and immutable afterwards, so queries
    /// only hold the lock long enough to clone the `Arc` and then sort/clone
    /// against the snapshot with no global lock held (§C1).
    data: ParkingMutex<Option<Arc<ScanData>>>,
    /// Set by `cancel_scan` while a walk is running.
    cancel: AtomicBool,
    /// Number of files met so far (walk thread owns the increments).
    ///
    /// The counters are a display-only readout; every happens-before edge
    /// that matters (state transitions, the published scan) rides on the
    /// mutexes below, so `Relaxed` suffices for all of them (§C7).
    files: AtomicU64,
    /// Number of directories met so far (walk threads own the increments).
    dirs: AtomicU64,
    /// Number of bytes of files met so far (walk threads own the increments).
    bytes: AtomicU64,
    /// Number of unreadable entries so far (walk threads own the increments).
    errors: AtomicU64,
    /// Name of the directory the walker most recently picked up.
    current: ParkingMutex<String>,
    /// User-facing snapshot of the engine, guarded by Tauri.
    state: ParkingMutex<ScanStateDto>,
    /// Children lists sorted by name/size, keyed by node id + sort tag
    /// (small LRU; also feeds the depth≥2 sub-entries — §C4).
    sorted_cache: ParkingMutex<LruCache<Arc<[u32]>>>,
    /// Bumped (Release) by `invalidate_sorted_cache` whenever `data`'s
    /// identity changes — scan start, cancel-clear, publish — immediately
    /// followed by a sorted-cache clear. Queries read it (Acquire) *before*
    /// snapshotting and re-check it under the cache lock before reading or
    /// inserting, so an entry computed from an old snapshot can never
    /// outlive a swap: node ids are dense and reused across scans, so a
    /// stale entry is indistinguishable from a valid one (§H race).
    generation: AtomicU64,
    /// When the running scan started, if any.
    started_at: ParkingMutex<Option<std::time::Instant>>,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            data: ParkingMutex::new(None),
            cancel: AtomicBool::new(false),
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            current: ParkingMutex::new(String::new()),
            state: ParkingMutex::new(ScanStateDto {
                state: "idle".into(),
                root: None,
                error: None,
            }),
            sorted_cache: ParkingMutex::new(LruCache::new(SORTED_CACHE_CAP)),
            generation: AtomicU64::new(0),
            started_at: ParkingMutex::new(None),
        }
    }

    /// Marks the engine as scanning and spawns the walker and the progress
    /// reporter. Fails if a scan is already running.
    pub fn start_scan(
        app: tauri::AppHandle,
        engine_state: tauri::State<'_, Arc<Self>>,
        root: String,
    ) -> Result<(), String> {
        struct TauriEvents(tauri::AppHandle);

        impl ScanEvents for TauriEvents {
            fn progress(&self, payload: ProgressPayload) {
                let _ = self.0.emit("scan://progress", payload);
            }

            fn done(&self, payload: DonePayload) {
                let _ = self.0.emit("scan://done", payload);
            }

            fn error(&self, payload: ErrorPayload) {
                let _ = self.0.emit("scan://error", payload);
            }
        }

        let engine = Arc::clone(engine_state.inner());
        Self::start_scan_events(&engine, Arc::new(TauriEvents(app)), root)
    }

    /// Same as [`Engine::start_scan`], but reports through any [`ScanEvents`]
    /// implementation instead of a Tauri handle.
    pub fn start_scan_events(
        engine: &Arc<Self>,
        events: Arc<dyn ScanEvents>,
        root: String,
    ) -> Result<(), String> {
        let mut state = engine.state.lock();

        if state.state == "scanning" {
            return Err("a scan is already in progress".to_string());
        }

        // Reset everything from any previous scan. The counters are a
        // display-only readout, so relaxed orderings are enough (§C7).
        engine.files.store(0, AtomicOrdering::Relaxed);
        engine.dirs.store(0, AtomicOrdering::Relaxed);
        engine.bytes.store(0, AtomicOrdering::Relaxed);
        engine.errors.store(0, AtomicOrdering::Relaxed);
        engine.cancel.store(false, AtomicOrdering::Relaxed);
        *engine.started_at.lock() = Some(std::time::Instant::now());
        engine.invalidate_sorted_cache();

        *state = ScanStateDto {
            state: "scanning".into(),
            root: Some(root.clone()),
            error: None,
        };
        drop(state);

        let done = Arc::new(AtomicBool::new(false));
        let walker_engine = Arc::clone(engine);
        let walker_done = Arc::clone(&done);
        let reporter_engine = Arc::clone(engine);
        let reporter_done = Arc::clone(&done);
        let panic_engine = Arc::clone(&walker_engine);
        let panic_done = Arc::clone(&walker_done);
        {
            let walker_events = Arc::clone(&events);
            let panic_events = Arc::clone(&events);
            std::thread::spawn(move || {
                // Coordinator thread: a panic here must surface as an error
                // event, never as a scan that never ends.
                let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    #[cfg(windows)]
                    run_walker_fast(walker_events, walker_engine, root, walker_done);
                    #[cfg(not(windows))]
                    run_walker(walker_events, walker_engine, root, walker_done);
                }));
                if caught.is_err() {
                    // A panic must never leave the UI waiting for events forever.
                    fatal(
                        &panic_engine,
                        panic_events.as_ref(),
                        "the scan crashed unexpectedly".to_string(),
                        panic_done,
                    );
                }
            });
        }
        {
            // The reporter sleeps most of its life and only reads Relaxed
            // counters + one mutex — no reason to prioritize it.
            std::thread::spawn(move || run_progress_reporter(events, reporter_engine, reporter_done));
        }
        // Both threads run detached: the command returns immediately and the
        // scan reports through the event callbacks.
        Ok(())
    }

    /// Stops the running walk at the next opportunity.
    pub fn cancel_scan(&self) {
        // Relaxed: the flag only needs to become visible to the walk
        // threads; the state it triggers is published under mutexes.
        self.cancel.store(true, AtomicOrdering::Relaxed);
    }

    /// Snapshot of the user-facing scan state.
    pub fn scan_state(&self) -> ScanStateDto {
        (*self.state.lock()).clone()
    }

    /// Clones the published scan's handle plus the cache generation in
    /// effect, holding the engine lock only for the clone. The generation is
    /// read BEFORE the snapshot: every identity change bumps it after the
    /// swap, so an observed bump (Acquire pairs with the publisher's Release)
    /// guarantees the snapshot below already sees the new tree. The snapshot
    /// is immutable afterwards, so sorting, cloning and response building all
    /// run with no global lock held (§C1).
    fn snapshot(&self) -> Result<(Arc<ScanData>, u64), String> {
        let scan_gen = self.generation.load(AtomicOrdering::Acquire);
        match self.data.lock().as_ref() {
            Some(data) => Ok((Arc::clone(data), scan_gen)),
            None => Err("no scan data".to_string()),
        }
    }

    /// Invalidates every sorted-cache entry for an imminent `data` identity
    /// change: bump the generation first, then clear under the cache lock.
    /// Queries take their generation check inside that same lock, so an
    /// in-flight query on the old tree either finishes its insert before this
    /// clear runs (the entry is removed) or fails the check (the insert is
    /// skipped) — both orders are safe.
    fn invalidate_sorted_cache(&self) {
        self.generation.fetch_add(1, AtomicOrdering::Release);
        *self.sorted_cache.lock() = LruCache::new(SORTED_CACHE_CAP);
    }

    /// Children of `node_id` as returned by `get_children`.
    pub fn get_children(
        &self,
        node_id: u32,
        depth: u32,
        sort: &str,
        limit: u32,
    ) -> Result<ChildrenResponse, String> {
        let (data, scan_gen) = self.snapshot()?;
        if node_id as usize >= data.nodes.len() {
            return Err("bad node".to_string());
        }

        let limit = if limit == 0 {
            DEFAULT_CHILDREN_LIMIT
        } else {
            limit
        };
        let total_entries = children_of(&data, node_id).len();

        let sort_tag = if sort == "size" { SORT_SIZE } else { SORT_NAME };
        let sorted = self.sorted_child_ids(&data, node_id, sort_tag, scan_gen);
        let mut entries: Vec<ChildEntry> = sorted
            .iter()
            .take(limit as usize)
            .map(|&id| build_entry(&data, id))
            .collect();

        if depth >= 2 {
            // The size-sorted sub-list of a directory is exactly the head of
            // the same cached sorted list the direct size query uses, so the
            // sub-entries ride on that cache entry instead of re-cloning and
            // re-sorting every directory on every call (§C4). Each list sorts
            // OUTSIDE the cache lock (see `sorted_child_ids`).
            for entry in entries.iter_mut() {
                if entry.is_dir {
                    let sub = self.sorted_child_ids(&data, entry.id, SORT_SIZE, scan_gen);
                    entry.sub = Some(
                        sub.iter()
                            .take(SUB_ENTRIES as usize)
                            .map(|&id| build_entry(&data, id))
                            .collect(),
                    );
                }
            }
        }

        // Breadcrumb chain from the root down to `node_id`.
        let mut chain_ids: Vec<u32> = Vec::new();
        let mut current = node_id;
        loop {
            chain_ids.push(current);
            let node = &data.nodes[current as usize];
            if node.parent == model::NO_PARENT {
                break;
            }
            current = node.parent;
        }
        chain_ids.reverse();
        let names: Vec<String> = chain_ids
            .iter()
            .map(|id| data.nodes[*id as usize].name.clone())
            .collect();
        let path = join_chain(&names);

        Ok(ChildrenResponse {
            parent_id: node_id,
            parent_size: data.nodes[node_id as usize].size,
            parent_cat: data.nodes[node_id as usize].cat,
            total_entries: total_entries as u32,
            entries,
            lineage: chain_ids
                .iter()
                .enumerate()
                .map(|(index, id)| model::LineageEntry {
                    id: *id,
                    name: names[index].clone(),
                })
                .collect(),
            path,
        })
    }

    /// The largest files of the finished scan.
    ///
    /// Full paths were materialized once at finalize (`top_paths`), so a call
    /// here is a plain zip of prepared data — the old per-call ancestor-chain
    /// walks are gone.
    pub fn get_largest_files(&self, limit: u32) -> Result<Vec<LargestFile>, String> {
        let (data, _gen) = self.snapshot()?;
        let take = if limit == 0 {
            usize::MAX
        } else {
            limit as usize
        };
        Ok(data
            .top_files
            .iter()
            .zip(&data.top_paths)
            .take(take)
            .map(|(&id, path)| {
                let node = &data.nodes[id as usize];
                LargestFile {
                    id,
                    name: node.name.clone(),
                    size: node.size,
                    cat: node.cat,
                    path: path.clone(),
                }
            })
            .collect())
    }

    /// Per-category byte totals of the subtree rooted at `node_id`.
    pub fn get_category_totals(&self, node_id: u32) -> Result<Vec<u64>, String> {
        // `finish_scan` persisted every node's subtree totals, so this is a
        // plain index instead of the old subtree DFS + cache dance (§C5).
        let (data, _gen) = self.snapshot()?;
        match data.dir_totals.get(node_id as usize) {
            Some(totals) => Ok(totals.to_vec()),
            None => Err("bad node".to_string()),
        }
    }

    /// Children of `node_id` sorted by `sort_tag`, through the LRU cache
    /// keyed by node id + sort tag (§C4: the size-sorted head of this exact
    /// list also answers the depth≥2 sub-entry queries).
    ///
    /// Two changes vs the old `cached_sorted`:
    /// - the SORT runs OUTSIDE the cache lock. A miss releases the lock,
    ///   sorts its own consistent snapshot, then re-locks only to insert
    ///   (generation-checked both times), so a slow first sort (an enormous
    ///   directory) no longer serializes every concurrent query;
    /// - entries are `Arc<[u32]>`: a hit clones a refcount, not a `Vec<u32>`.
    ///
    /// `scan_gen` is the caller's generation check: when the snapshot
    /// predates a `data` swap, the cache is neither read — its entries belong
    /// to the newer tree and may be out of range for this snapshot — nor
    /// written — the entry would outlive the swap, and since node ids are
    /// dense and reused across scans, staleness would be indistinguishable
    /// from validity. The un-cached sort still answers this one request from
    /// the caller's own consistent snapshot.
    fn sorted_child_ids(
        &self,
        data: &ScanData,
        node_id: u32,
        sort_tag: u64,
        scan_gen: u64,
    ) -> Arc<[u32]> {
        let key = ((node_id as u64) << 2) | sort_tag;
        // Hit path: generation check INSIDE the cache lock, same contract as
        // the old implementation.
        {
            let mut cache = self.sorted_cache.lock();
            if self.generation.load(AtomicOrdering::Relaxed) == scan_gen {
                if let Some(hit) = cache.get(key) {
                    return hit;
                }
            }
        }
        // Miss: sort with no cache lock held.
        let mut ids = children_of(data, node_id).to_vec();
        sort_child_ids(data, &mut ids, sort_tag == SORT_SIZE);
        let ids: Arc<[u32]> = ids.into();
        let mut cache = self.sorted_cache.lock();
        if self.generation.load(AtomicOrdering::Relaxed) == scan_gen {
            cache.insert(key, Arc::clone(&ids));
        }
        ids
    }
}

// ---------------------------------------------------------------------------
// staging: bulk appends into the shared store
// ---------------------------------------------------------------------------

/// Moves a worker's staged buffers into the shared store; returns the id the
/// first appended node received (`base`) plus the (files, bytes) display
/// counters for the batch. The critical section is a handful of bulk `extend`s
/// — no per-entry locks, no hashing, no `DashMap` shard contention (PERF item
/// 4; the old per-directory `children_map` insert built one heap `Vec<u32>`
/// per directory inside the sharded map).
///
/// Ids are assigned here from `nodes.len()` (under the lock), preserving the
/// dense `nodes[id]` identity that the whole query layer relies on. A caller
/// that staged subdirectories learns their global ids from `base`: staged
/// index + `base` is exactly the node id written above, so a subdirectory's id
/// is already in the store before its work item can be popped by a peer.
fn append_to_store(
    arena: &mut Vec<u16>,
    nodes: &mut Vec<Node>,
    spans: &mut Vec<(u32, u32)>,
    pairs: &mut Vec<ChildPair>,
    top: &mut Vec<(u64, u32)>,
    store: &SharedStore,
) -> (u32, u64, u64) {
    if nodes.is_empty() {
        return (0, 0, 0);
    }
    let mut inner = store.lock();
    let base = inner.nodes.len() as u32;
    let arena_base = inner.arena.len() as u32;

    // Top-file candidates: (size, id) pairs recorded once per batch with ids
    // assigned here. Kept per worker — zero per-file heap locking (PERF item
    // 3). A file only leaves this list when the list is longer than the
    // global cap, in which case 1000 strictly larger files exist in this very
    // worker — exactly the old heap's eviction argument.
    let mut file_count = 0u64;
    let mut byte_count = 0u64;
    for (index, node) in nodes.iter().enumerate() {
        if node.is_dir {
            continue;
        }
        file_count += 1;
        byte_count += node.size;
        top.push((node.size, base + index as u32));
    }
    if top.len() > TOP_FILES_RESIZE {
        top.sort_by(|a, b| b.0.cmp(&a.0));
        top.truncate(TOP_FILES_CAP);
    }

    inner.nodes.extend(nodes.drain(..));
    inner.arena.extend(arena.drain(..));
    inner.spans.extend(
        spans
            .drain(..)
            .map(|(offset, len)| (offset + arena_base, len)),
    );
    inner
        .child_pairs
        .extend(pairs.drain(..).map(|(parent, index)| (parent, base + index)));
    (base, file_count, byte_count)
}

/// Name-byte encode helpers. Windows pushes real UTF-16 (it already has it
/// from the find data); other platforms push each raw `OsStr` byte
/// zero-extended, and materialization lossy-decodes those bytes back — the
/// same result the old `to_string_lossy` produced.
#[cfg(not(windows))]
fn push_name_bytes(inner: &mut StoreInner, bytes: &[u8]) -> (u32, u32) {
    let offset = inner.arena.len() as u32;
    inner.arena.extend(bytes.iter().map(|&byte| byte as u16));
    (offset, bytes.len() as u32)
}

/// Materializes a node's name from the walk-time arena. Runs once per node at
/// finalize — one bulk pass, never in the per-file hot loop.
#[cfg(windows)]
fn name_from_arena(arena: &[u16], span: (u32, u32)) -> String {
    String::from_utf16_lossy(&arena[span.0 as usize..span.0 as usize + span.1 as usize])
}

#[cfg(not(windows))]
fn name_from_arena(arena: &[u16], span: (u32, u32)) -> String {
    let bytes: Vec<u8> = arena[span.0 as usize..span.0 as usize + span.1 as usize]
        .iter()
        .map(|&unit| unit as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

// ---------------------------------------------------------------------------
// non-Windows fallback: jwalk walk (kept as-is, parallel via rayon)
// ---------------------------------------------------------------------------

/// The element type of `jwalk`'s client state: an empty read-dir state and an
/// empty per-entry state.
#[cfg(not(windows))]
type JwalkClientState = ((), ());
/// A directory entry as handed to the walk callback.
#[cfg(not(windows))]
type JwalkEntry = DirEntry<JwalkClientState>;
/// Result of reading one directory.
#[cfg(not(windows))]
type WalkedResult = Result<JwalkEntry, jwalk::Error>;

/// Walks `root` and fills the engine arena (jwalk fallback for non-Windows).
///
/// Runs on its own thread: the `jwalk` iterator is driven (with its default
/// rayon parallelism) one directory listing at a time; each listing stages
/// into a local `WorkerCtx` and lands in the shared store as one bulk append.
#[cfg(not(windows))]
fn run_walker(
    events: Arc<dyn ScanEvents>,
    engine: Arc<Engine>,
    root_path: String,
    done: Arc<AtomicBool>,
) {
    if std::fs::read_dir(&root_path).is_err() {
        return fatal(
            engine.as_ref(),
            events.as_ref(),
            "cannot read root directory".to_string(),
            done,
        );
    }

    let store: SharedStore = Arc::new(ParkingMutex::new(StoreInner::default()));
    let dir_ids: Arc<DashMap<PathBuf, u32>> = Arc::new(DashMap::new());
    let top: Arc<ParkingMutex<Vec<(u64, u32)>>> =
        Arc::new(ParkingMutex::new(Vec::with_capacity(TOP_FILES_CAP)));

    let root_id = 0u32;
    {
        let mut inner = store.lock();
        let (offset, len) = push_name_bytes(&mut inner, root_path.as_bytes());
        inner.spans.push((offset, len));
        inner.nodes.push(Node {
            name: String::new(),
            parent: model::NO_PARENT,
            is_dir: true,
            size: 0,
            file_count: 0,
            cat: model::CAT_OTHER,
            kid_count: 0,
        });
        dir_ids.insert(PathBuf::from(root_path.clone()), root_id);
    }

    let walker = WalkDir::new(&root_path)
        .skip_hidden(false)
        .follow_links(false)
        .process_read_dir({
            let engine = Arc::clone(&engine);
            let dir_ids = Arc::clone(&dir_ids);
            let top = Arc::clone(&top);
            let store = Arc::clone(&store);
            move |depth: Option<usize>,
                  dir_path: &Path,
                  _state: &mut (),
                  children: &mut Vec<WalkedResult>| {
                walk_callback(
                    &engine,
                    depth,
                    dir_path,
                    _state,
                    children,
                    &dir_ids,
                    &store,
                    &top,
                )
            }
        });

    for entry in walker {
        // Entries yielded as errors (unreadable subdirectories) are counted by
        // the callback itself; the root was already checked.
        let _ = entry;
    }

    if engine.cancel.load(AtomicOrdering::Relaxed) {
        // Cancelled: silently return to idle, the frontend resets its own UI.
        *engine.state.lock() = ScanStateDto {
            state: "idle".into(),
            root: None,
            error: None,
        };
        *engine.data.lock() = None;
        engine.invalidate_sorted_cache();
        done.store(true, AtomicOrdering::Relaxed);
        return;
    }

    let elapsed_ms = elapsed_ms(&engine);
    let tops = vec![std::mem::take(&mut *top.lock())];
    finish_scan(
        &engine,
        events.as_ref(),
        root_path,
        elapsed_ms,
        store,
        tops,
    );
    done.store(true, AtomicOrdering::Relaxed);
}

/// Processes one directory listing and appends its entries.
#[cfg(not(windows))]
#[allow(clippy::too_many_arguments)]
fn walk_callback(
    engine: &Engine,
    depth: Option<usize>,
    dir_path: &Path,
    _state: &mut (),
    children: &mut Vec<WalkedResult>,
    dir_ids: &DashMap<PathBuf, u32>,
    store: &SharedStore,
    top: &ParkingMutex<Vec<(u64, u32)>>,
) {
    // Cancellation: prune every descent and stop building the tree.
    if engine.cancel.load(AtomicOrdering::Relaxed) {
        for child in children.iter_mut() {
            if let WalkedResult::Ok(entry) = child {
                if entry.file_type().is_dir() {
                    entry.read_children_path = None;
                }
            }
        }
        children.clear();
        return;
    }

    // `jwalk` invokes this callback once for the root itself with `depth:
    // None`, handing the root entry over as its only "child" (the path is the
    // root's parent). The root node already exists, so there is nothing to
    // append for that virtual listing.
    if depth.is_none() {
        return;
    }

    // `DashMap<PathBuf, _>` is borrowed as `Path` (`PathBuf: Borrow<Path>`),
    // which keeps the per-directory `PathBuf` clone out of the hot loop.
    let dir_id = match dir_ids.get(dir_path) {
        Some(guard) => *guard.value(),
        None => return,
    };

    // Only the shallow levels update the progress "current" path: the
    // full-path String allocation would otherwise be paid for every deep
    // directory, feeding a readout nobody can read. Shallow directories are
    // visited first, so every progress tick still shows a real path.
    if depth.is_some_and(|d| d <= 2) {
        *engine.current.lock() = dir_path.to_string_lossy().into_owned();
    }

    // Directory-level classification flags (in-cache-dir? under-C:\Windows?)
    // computed once per directory and reused for every file in it — now
    // allocation-free as well (see classify::dir_flags).
    let flags = classify::dir_flags(dir_path);

    let mut ctx = WorkerCtx::default();
    let mut index = 0;
    while index < children.len() {
        match children.get_mut(index) {
            Some(WalkedResult::Ok(entry)) => {
                handle_child(engine, entry, dir_id, flags, store, dir_ids, &mut ctx);
                index += 1;
            }
            Some(WalkedResult::Err(_)) => {
                engine.errors.fetch_add(1, AtomicOrdering::Relaxed);
                index += 1;
            }
            None => return,
        }
    }

    // One bulk append per listing (the listing runs on some rayon worker; the
    // staged buffers are thread-local until this point). Files/bytes counters
    // ride along instead of an atomic add per file.
    let (_base, files, bytes) = {
        let mut top_guard = top.lock();
        append_to_store(
            &mut ctx.arena,
            &mut ctx.nodes,
            &mut ctx.spans,
            &mut ctx.pairs,
            &mut top_guard,
            store,
        )
    };
    engine.files.fetch_add(files, AtomicOrdering::Relaxed);
    engine.bytes.fetch_add(bytes, AtomicOrdering::Relaxed);
}

/// Builds one child node, staging it (files) or registering it immediately
/// (directories — their id must exist before their listing runs, which is why
/// directory registration takes the short store lock here).
#[cfg(not(windows))]
fn handle_child(
    engine: &Engine,
    entry: &mut JwalkEntry,
    parent_id: u32,
    flags: classify::DirFlags,
    store: &SharedStore,
    dir_ids: &DashMap<PathBuf, u32>,
    ctx: &mut WorkerCtx,
) {
    if entry.file_type().is_symlink() {
        // Skip symlinks entirely: never follow them, never count them.
        return;
    }
    let metadata = match entry.metadata() {
        Ok(metadata) => metadata,
        Err(_) => {
            engine.errors.fetch_add(1, AtomicOrdering::Relaxed);
            return;
        }
    };

    let is_dir = entry.file_type().is_dir();
    // The name's raw bytes go straight into the staging arena (the only
    // allocation left per file here is jwalk's internal entry bookkeeping);
    // the old per-file `to_string_lossy().to_string()` double allocation and
    // the lowercased extension are both gone.
    let name = entry.file_name();
    let bytes = name.as_encoded_bytes();
    let cat = if is_dir {
        model::CAT_OTHER
    } else {
        classify::file_category_bytes(bytes, flags)
    };

    if is_dir {
        let child_id = {
            let mut inner = store.lock();
            let id = inner.nodes.len() as u32;
            let (offset, len) = push_name_bytes(&mut inner, bytes);
            inner.spans.push((offset, len));
            inner.child_pairs.push((parent_id, id));
            inner.nodes.push(Node {
                name: String::new(),
                parent: parent_id,
                is_dir: true,
                size: 0,
                file_count: 0,
                cat: model::CAT_OTHER,
                kid_count: 0,
            });
            id
        };
        engine.dirs.fetch_add(1, AtomicOrdering::Relaxed);
        // jwalk will list this directory later and resolves its node id
        // through this map (same registration the old append_batch did).
        dir_ids.insert(entry.path(), child_id);
    } else {
        let offset = ctx.arena.len() as u32;
        ctx.arena.extend(bytes.iter().map(|&byte| byte as u16));
        ctx.spans.push((offset, bytes.len() as u32));
        ctx.pairs.push((parent_id, ctx.nodes.len() as u32));
        ctx.nodes.push(Node {
            name: String::new(),
            parent: parent_id,
            is_dir: false,
            size: metadata.len(),
            file_count: 1,
            cat,
            kid_count: 0,
        });
    }
}

// ---------------------------------------------------------------------------
// progress reporting
// ---------------------------------------------------------------------------

/// Reports scan progress every [`PROGRESS_INTERVAL`] until the scan is done.
/// All counters are read `Relaxed` (display-only readout, §C7); the tick
/// cadence is unchanged at 120 ms.
fn run_progress_reporter(events: Arc<dyn ScanEvents>, engine: Arc<Engine>, done: Arc<AtomicBool>) {
    loop {
        std::thread::sleep(PROGRESS_INTERVAL);
        if done.load(AtomicOrdering::Relaxed) {
            break;
        }
        let payload = ProgressPayload {
            files: engine.files.load(AtomicOrdering::Relaxed),
            dirs: engine.dirs.load(AtomicOrdering::Relaxed),
            bytes: engine.bytes.load(AtomicOrdering::Relaxed),
            errors: engine.errors.load(AtomicOrdering::Relaxed),
            current: engine.current.lock().clone(),
            elapsed_ms: elapsed_ms(&engine),
        };
        events.progress(payload);
    }
}

/// Fails the scan fatally: reports the error and stops.
pub(crate) fn fatal(
    engine: &Engine,
    events: &dyn ScanEvents,
    message: String,
    done: Arc<AtomicBool>,
) {
    *engine.state.lock() = ScanStateDto {
        state: "error".into(),
        root: None,
        error: Some(message.clone()),
    };
    events.error(ErrorPayload { message });
    done.store(true, AtomicOrdering::Relaxed);
}

/// Milliseconds elapsed since the scan started.
fn elapsed_ms(engine: &Engine) -> u64 {
    let started = engine.started_at.lock();
    match &*started {
        Some(started) => started.elapsed().as_millis() as u64,
        None => 0,
    }
}

// ---------------------------------------------------------------------------
// finalize
// ---------------------------------------------------------------------------

/// Builds the finished scan and publishes it, then emits `scan://done`.
///
/// This is the single pass where walk-time staging becomes query-ready data:
/// names are materialized (one allocation per node, on the rayon pool — the
/// hot walk never allocated a name), the (parent, child) log becomes one flat
/// CSR children index in three O(n) passes, and the per-worker top lists merge
/// into the global top-1000 with one sort.
fn finish_scan(
    engine: &Engine,
    events: &dyn ScanEvents,
    root_path: String,
    elapsed_ms: u64,
    store: SharedStore,
    tops: Vec<Vec<(u64, u32)>>,
) {
    let StoreInner {
        mut nodes,
        arena,
        spans,
        child_pairs,
    } = {
        let mut guard = store.lock();
        std::mem::take(&mut *guard)
    };

    let node_count = nodes.len();
    let total_dirs = engine.dirs.load(AtomicOrdering::Relaxed);
    let total_errors = engine.errors.load(AtomicOrdering::Relaxed);

    // Materialize names: one allocation per node, spread over the rayon pool.
    // This is the only place names come into existence (the walk staged raw
    // UTF-16), so on a multi-million-entry tree it is a pure N-malloc pass —
    // exactly the kind of work that parallelizes for free. The span lookup is
    // bounds-checked, so a malformed staging buffer degrades to an empty name
    // instead of panicking.
    nodes
        .par_iter_mut()
        .enumerate()
        .for_each(|(index, node)| {
            if let Some(&span) = spans.get(index) {
                node.name = name_from_arena(&arena, span);
            }
        });
    // The arena is dead weight after materialization.
    drop(arena);
    drop(spans);

    // Children index: one flat CSR pair built from the (parent, child) log in
    // three O(n) passes (count → prefix sum → fill); node ids are dense, so
    // there is nothing to hash. Replaces `Vec<Vec<u32>>`, which allocated at
    // least one heap block PER DIRECTORY (plus a 24-byte header each) yet was
    // only ever read as a slice.
    let mut child_offsets: Vec<u32> = vec![0u32; node_count + 1];
    for &(parent, _) in &child_pairs {
        let parent = parent as usize;
        if parent < node_count {
            child_offsets[parent + 1] += 1;
        }
    }
    for index in 0..node_count {
        child_offsets[index + 1] += child_offsets[index];
    }
    let mut child_ids: Vec<u32> = vec![0u32; child_offsets[node_count] as usize];
    {
        // Fill cursor: one slot per node, advanced as its children land.
        let mut cursor = child_offsets.clone();
        for &(parent, child) in &child_pairs {
            let parent = parent as usize;
            if parent < node_count {
                let slot = cursor[parent] as usize;
                child_ids[slot] = child;
                cursor[parent] += 1;
            }
        }
    }
    drop(child_pairs);
    for (index, node) in nodes.iter_mut().enumerate() {
        node.kid_count = child_offsets[index + 1] - child_offsets[index];
    }

    // Reverse pass: every node is visited after all of its children (a child
    // always owns a higher id than its parent — ids are handed out after the
    // parent's listing has started), so its subtree totals are already
    // complete when it is reached. Sizes and file counts are folded into the
    // parent; the dominant category of a directory replaces its placeholder
    // and the per-category bytes bubble up as well.
    //
    // The accumulator is a plain Vec indexed by node id (§C5): node ids are
    // dense and already topological, so there is nothing to hash. Each
    // directory's finished `[u64; 8]` is kept — it is published inside
    // `ScanData` so `get_category_totals` never walks a subtree again.
    let mut dir_totals: Vec<[u64; 8]> = vec![[0u64; 8]; node_count];
    let mut categories = [0u64; 8];
    for index in (0..node_count).rev() {
        let (parent, size, files, is_dir, cat) = {
            let node = &nodes[index];
            (
                node.parent,
                node.size,
                node.file_count,
                node.is_dir,
                node.cat,
            )
        };

        if parent != model::NO_PARENT {
            let slot = &mut nodes[parent as usize];
            slot.size += size;
            slot.file_count += files;
        }

        if is_dir {
            let own = dir_totals[index];
            nodes[index].cat = dominant_category(&own);
            match parent {
                model::NO_PARENT => categories = own,
                _ => {
                    let entry = &mut dir_totals[parent as usize];
                    for cat in 0..8 {
                        entry[cat] += own[cat];
                    }
                }
            }
        } else {
            dir_totals[parent as usize][cat as usize] += size;
            // Mirror the file's own bucket as well: the old
            // `get_category_totals(file_id)` answered with the file's own
            // bytes, and the persisted vector must answer identically.
            dir_totals[index][cat as usize] = size;
        }
    }

    // Merge every worker's local top list: one sort over the concatenation,
    // then the single global truncation. (PERF item 3: the per-file shared
    // heap lock is gone; workers also already bounded their own lists.)
    let mut all_top: Vec<(u64, u32)> = tops.into_iter().flatten().collect();
    all_top.sort_by(|a, b| b.0.cmp(&a.0));
    all_top.truncate(TOP_FILES_CAP);
    let top_files: Vec<u32> = all_top.iter().map(|&(_, id)| id).collect();

    // Full paths for the top list materialized ONCE here, so
    // `get_largest_files` never walks ancestor chains per request (PERF item
    // 9). LargestFile keeps its exact DTO shape.
    let top_paths: Vec<String> = top_files
        .iter()
        .map(|&id| node_full_path(&nodes, id))
        .collect();

    let root_size = nodes[0].size;
    let root_files = nodes[0].file_count;
    let scan = ScanData {
        root_id: 0,
        root_path,
        nodes,
        child_offsets,
        child_ids,
        dir_totals,
        categories: categories.to_vec(),
        top_files,
        top_paths,
        total_bytes: root_size,
        total_files: root_files,
        total_dirs,
        errors: total_errors,
        elapsed_ms,
    };

    let root_name = engine_root(&engine);
    let payload = DonePayload {
        root: root_name.clone(),
        root_id: 0,
        total_bytes: scan.total_bytes,
        total_files: scan.total_files,
        total_dirs: scan.total_dirs,
        errors: scan.errors,
        elapsed_ms,
        categories: scan.categories.clone(),
    };
    // Published as an immutable snapshot: queries clone the Arc under the
    // lock and work lock-free afterwards (§C1). The generation bump +
    // sorted-cache clear immediately after the swap closes the §H race: any
    // in-flight query still holding the old tree read the old generation
    // before its snapshot, so it now fails its in-lock check and can neither
    // serve nor re-insert stale entries.
    *engine.data.lock() = Some(Arc::new(scan));
    engine.invalidate_sorted_cache();
    *engine.state.lock() = ScanStateDto {
        state: "ready".into(),
        root: Some(root_name),
        error: None,
    };
    events.done(payload);
}

/// Reads the root path back from the engine state, which still carries it.
fn engine_root(engine: &Engine) -> String {
    engine
        .state
        .lock()
        .root
        .clone()
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// query-layer helpers
// ---------------------------------------------------------------------------

/// Small LRU cache: entries live in a vector, most recently used go last.
///
/// Keys are `(node id, sort tag)` packed into a `u64` (§C4). Values are
/// `Arc<[u32]>` so a hit is one refcount clone, never a fresh `Vec<u32>`.
struct LruCache<V> {
    entries: Vec<(u64, V)>,
    cap: usize,
}

impl<V: Clone> LruCache<V> {
    fn new(cap: usize) -> Self {
        Self {
            entries: Vec::new(),
            cap,
        }
    }

    /// Returns a clone of the value for `key`, marking the entry as recently used.
    fn get(&mut self, key: u64) -> Option<V> {
        let index = self
            .entries
            .iter()
            .position(|(id, _)| *id == key)?;
        let value = self.entries.remove(index).1;
        self.entries.push((key, value.clone()));
        Some(value)
    }

    fn insert(&mut self, key: u64, value: V) {
        while self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        self.entries.push((key, value));
    }
}

/// The category id that dominates the byte totals of a directory.
fn dominant_category(totals: &[u64; 8]) -> u8 {
    let mut best = model::CAT_OTHER as usize;
    let mut best_size = 0u64;
    for (cat, bytes) in totals.iter().enumerate() {
        if *bytes > best_size {
            best_size = *bytes;
            best = cat;
        }
    }
    best as u8
}

/// Joins a root-down chain of names into one display path.
fn join_chain(names: &[String]) -> String {
    let mut path = PathBuf::new();
    for name in names {
        path.push(name);
    }
    path.to_string_lossy().into_owned()
}

/// Full path of `id`, rebuilt from the root down.
fn node_full_path(nodes: &[Node], id: u32) -> String {
    let mut chain = Vec::new();
    let mut current = id;
    loop {
        let node = &nodes[current as usize];
        chain.push(node.name.clone());
        if node.parent == model::NO_PARENT {
            break;
        }
        current = node.parent;
    }
    chain.reverse();
    join_chain(&chain)
}

/// Byte-wise case-insensitive name comparison. Zero allocations: the old
/// `sort_child_ids` built one lowercased `String` per child (O(n) heap traffic
/// per sorted directory); this folds bytes in place. For ASCII names — the
/// overwhelmingly common case — the ordering is the lowered-string ordering:
/// equal raw bytes compare equal, differing bytes compare by their ASCII
/// fold, case-only differences keep scanning, lengths break the tie.
fn cmp_name_ci(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let common = a.len().min(b.len());
    for index in 0..common {
        let (x, y) = (a[index], b[index]);
        if x != y {
            let (xl, yl) = (x.to_ascii_lowercase(), y.to_ascii_lowercase());
            if xl != yl {
                return xl.cmp(&yl);
            }
        }
    }
    a.len().cmp(&b.len())
}

/// Directory-before-file tie-break (the old compare_by_name rule).
fn dir_first(nodes: &[Node], a: u32, b: u32) -> Ordering {
    match (nodes[a as usize].is_dir, nodes[b as usize].is_dir) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// Compares two children by size, biggest first; directories sort first on
/// ties. The name tie-break lives in [`sort_child_ids`] (allocation-free).
fn compare_by_size(data: &ScanData, a: u32, b: u32) -> Ordering {
    let a_node = &data.nodes[a as usize];
    let b_node = &data.nodes[b as usize];
    match b_node.size.cmp(&a_node.size) {
        Ordering::Equal => match (a_node.is_dir, b_node.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => Ordering::Equal,
        },
        other => other,
    }
}

/// Sort tag of the size-sorted lists inside the sorted-children LRU.
const SORT_SIZE: u64 = 0;
/// Sort tag of the name-sorted lists inside the sorted-children LRU.
const SORT_NAME: u64 = 1;

/// Sorts `ids` in place by size (biggest first, directories first on ties)
/// or by name (case-insensitive, directories first).
///
/// Comparator-based with precomputed-key allocations removed: the old version
/// materialized a lowercase `String` per child (two heap traffic generators:
/// the keys Vec and every grow), this walks the name bytes inside the
/// comparator, so a sort allocates nothing. Ties fall back to directories
/// first and then to the sort's own stability (same tie classes as before).
fn sort_child_ids(data: &ScanData, ids: &mut [u32], by_size: bool) {
    if by_size {
        ids.sort_by(|&a, &b| {
            match compare_by_size(data, a, b) {
                Ordering::Equal => {}
                other => return other,
            }
            match cmp_name_ci(&data.nodes[a as usize].name, &data.nodes[b as usize].name) {
                Ordering::Equal => dir_first(&data.nodes, a, b),
                other => other,
            }
        });
    } else {
        ids.sort_by(|&a, &b| {
            match cmp_name_ci(&data.nodes[a as usize].name, &data.nodes[b as usize].name) {
                Ordering::Equal => dir_first(&data.nodes, a, b),
                other => other,
            }
        });
    }
}

/// Builds the `ChildEntry` for one node. Sub-entries are attached by the caller.
fn build_entry(data: &ScanData, node_id: u32) -> ChildEntry {
    let node = &data.nodes[node_id as usize];
    let kid_count = children_of(data, node_id).len();
    ChildEntry {
        id: node_id,
        name: node.name.clone(),
        size: node.size,
        is_dir: node.is_dir,
        file_count: node.file_count,
        cat: node.cat,
        kid_count: kid_count as u32,
        sub: None,
    }
}

// ---------------------------------------------------------------------------
// Windows fast path: direct FindFirstFileExW enumeration (cfg windows).
// ---------------------------------------------------------------------------

#[cfg(windows)]
struct WorkItem {
    /// Directory path as UTF-16 (no trailing separator). Carrying UTF-16
    /// keeps the whole pipeline allocation-light: the search pattern is
    /// built by extending a buffer, `dir_flags` parses the UTF-16 directly,
    /// and no `PathBuf`/`OsString` round-trip happens per directory.
    path: Vec<u16>,
    /// Node id of the directory, assigned when its batch is appended to the
    /// store — always BEFORE the item is enqueued, so every child has a valid
    /// parent id by the time any worker starts listing this directory.
    id: u32,
    depth: u32,
    /// Classification flags of THIS directory, inherited from the parent plus
    /// its own name (`classify::child_flags`) — the full path is never
    /// re-scanned per directory.
    flags: classify::DirFlags,
}

/// A subdirectory found while listing its parent, staged in the worker's
/// buffers together with its node.
#[cfg(windows)]
struct StagedSubdir {
    /// Child path (`parent + '\' + name`) as UTF-16, ready for
    /// `FindFirstFileExW`.
    path: Vec<u16>,
    /// Index inside `WorkerCtx::nodes` while staged; rebased to the global
    /// node id (`base + index`) when the batch is appended to the store.
    index: u32,
    /// The child's classification flags (parent flags + this name).
    flags: classify::DirFlags,
}

/// Retires one in-flight directory. `active` counts queued + in-flight items;
/// reaching zero is the single "walk is over" transition (notified on both
/// condvars: parked workers exit, the coordinator stops waiting). Every other
/// retirement is silent — workers park only on an empty queue, so queued work
/// always has a live drainer, and the completion condition can never be
/// starved (PERF item 5).
#[cfg(windows)]
fn retire(active: &AtomicU32, work_cv: &Condvar, done_cv: &Condvar) {
    if active.fetch_sub(1, AtomicOrdering::Release) == 1 {
        work_cv.notify_all();
        done_cv.notify_all();
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
pub fn run_walker_fast(
    events: Arc<dyn ScanEvents>,
    engine: Arc<Engine>,
    root_path: String,
    done: Arc<AtomicBool>,
) {
    if std::fs::read_dir(&root_path).is_err() {
        return fatal(
            &engine,
            events.as_ref(),
            "cannot read root directory".to_string(),
            done,
        );
    }

    let store: SharedStore = Arc::new(ParkingMutex::new(StoreInner::default()));
    {
        let mut inner = store.lock();
        let offset = inner.arena.len() as u32;
        inner.arena.extend(root_path.encode_utf16());
        let len = inner.arena.len() as u32 - offset;
        inner.spans.push((offset, len));
        inner.nodes.push(Node {
            name: String::new(),
            parent: model::NO_PARENT,
            is_dir: true,
            size: 0,
            file_count: 0,
            cat: model::CAT_OTHER,
            kid_count: 0,
        });
    }

    // Queue + wakeup plumbing. Two condvars so a `notify_one` on a push can
    // never be eaten by the coordinator: workers park on `work_cv`, the
    // coordinator waits on `done_cv`, and the walk-over transition notifies
    // both.
    let queue: Arc<ParkingMutex<VecDeque<WorkItem>>> =
        Arc::new(ParkingMutex::new(VecDeque::new()));
    let work_cv: Arc<Condvar> = Arc::new(Condvar::new());
    let done_cv: Arc<Condvar> = Arc::new(Condvar::new());
    let active: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    // Workers parked on `work_cv`. Producers consult it (under the queue lock,
    // which its increments/decrements also run under) to skip notifies nobody
    // is waiting for.
    let parked: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));

    {
        let mut q = queue.lock();
        let root_utf16: Vec<u16> = root_path.encode_utf16().collect();
        let root_flags = classify::dir_flags_utf16(&root_utf16);
        q.push_back(WorkItem {
            path: root_utf16,
            id: 0,
            depth: 0,
            flags: root_flags,
        });
        active.store(1, AtomicOrdering::Relaxed);
        // No notify: the workers are spawned right below and pick this up
        // immediately.
    }

    // One worker per logical processor (capped): directory enumeration is
    // kernel-bound, so extra threads hide per-directory syscall latency up to
    // a point — past ~16 they only fight over the queue and the store lock.
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(16)
        .max(2);

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let queue = Arc::clone(&queue);
        let work_cv = Arc::clone(&work_cv);
        let done_cv = Arc::clone(&done_cv);
        let active = Arc::clone(&active);
        let parked = Arc::clone(&parked);
        let engine = Arc::clone(&engine);
        let store = Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            // Below-normal priority for every scan worker: during a scan the
            // UI thread and webview keep their scheduling slices (PERF item
            // 7). `GetCurrentThread` is the calling thread's pseudo-handle.
            unsafe {
                SetThreadPriority(
                    GetCurrentThread(),
                    THREAD_PRIORITY_BELOW_NORMAL,
                );
            }
            let mut ctx = WorkerCtx::default();
            loop {
                // Drain up to POP_BATCH items per wake: the queue is fed one
                // directory listing at a time, so this keeps the number of
                // lock round-trips and futex sleeps (and wake stampedes) low.
                let mut items: Vec<WorkItem> = Vec::new();
                {
                    let mut q = queue.lock();
                    if q.is_empty() {
                        if active.load(AtomicOrdering::Acquire) == 0 {
                            break;
                        }
                        // Park while holding the queue lock: a producer that
                        // pushes afterwards observes `parked` under the same
                        // lock, so a push always reaches exactly the workers
                        // that are really waiting (no lost wakeups, no
                        // notifies into the void).
                        parked.fetch_add(1, AtomicOrdering::Relaxed);
                        work_cv.wait(&mut q);
                        parked.fetch_sub(1, AtomicOrdering::Relaxed);
                        continue;
                    }
                    let take = q.len().min(POP_BATCH);
                    items.reserve(take);
                    for _ in 0..take {
                        if let Some(item) = q.pop_front() {
                            items.push(item);
                        }
                    }
                }
                for item in items {
                    // Each directory body is panic-isolated; `active` is
                    // retired on EVERY path below, so a panicking worker can
                    // never hang the walk's completion condition (PERF item
                    // 6 — the old code could deadlock exactly this way).
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        process_dir_fast(
                            &engine, &store, &queue, &work_cv, &active, &parked, item, &mut ctx,
                        );
                    }));
                    retire(&active, &work_cv, &done_cv);
                }
            }
            // Hand any unfinished staging to the store, then return the
            // worker's top list through the thread's join value: merging it
            // needs no lock at all (PERF item 3).
            if !engine.cancel.load(AtomicOrdering::Relaxed) {
                flush_worker(&engine, &mut ctx, &store);
            }
            ctx.top.sort_by(|a, b| b.0.cmp(&a.0));
            ctx.top.truncate(TOP_FILES_CAP);
            ctx.top
        }));
    }

    {
        let mut q = queue.lock();
        while active.load(AtomicOrdering::Acquire) > 0 || !q.is_empty() {
            if engine.cancel.load(AtomicOrdering::Relaxed) {
                q.clear();
            }
            done_cv.wait(&mut q);
        }
    }
    let tops: Vec<Vec<(u64, u32)>> = handles
        .into_iter()
        .map(|h| h.join().unwrap_or_default())
        .collect();

    if engine.cancel.load(AtomicOrdering::Relaxed) {
        *engine.state.lock() = ScanStateDto {
            state: "idle".into(),
            root: None,
            error: None,
        };
        *engine.data.lock() = None;
        engine.invalidate_sorted_cache();
        done.store(true, AtomicOrdering::Relaxed);
        return;
    }

    let elapsed = elapsed_ms(&engine);
    finish_scan(
        &engine,
        events.as_ref(),
        root_path,
        elapsed,
        store,
        tops,
    );
    done.store(true, AtomicOrdering::Relaxed);
}

/// Flushes a worker's staged buffers and feeds the display counters.
#[cfg(windows)]
fn flush_worker(engine: &Engine, ctx: &mut WorkerCtx, store: &SharedStore) {
    let (_base, files, bytes) = append_to_store(
        &mut ctx.arena,
        &mut ctx.nodes,
        &mut ctx.spans,
        &mut ctx.pairs,
        &mut ctx.top,
        store,
    );
    engine.files.fetch_add(files, AtomicOrdering::Relaxed);
    engine.bytes.fetch_add(bytes, AtomicOrdering::Relaxed);
}

#[inline]
#[cfg(windows)]
fn wide_name(buf: &[u16; 260]) -> &[u16] {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    &buf[..len]
}

/// Short RAII guard so a panic (or early return) can never leak a find
/// handle. Each worker usually wraps its whole body in `catch_unwind`
/// anyway, but leaked OS handles would outlive the scan.
#[cfg(windows)]
struct FindGuard(HANDLE);

#[cfg(windows)]
impl Drop for FindGuard {
    fn drop(&mut self) {
        unsafe { FindClose(self.0); }
    }
}

/// Publishes everything a worker has staged: ONE store append (which assigns
/// the global ids of the staged subdirectories) followed by ONE queue push of
/// exactly those subdirectories.
///
/// Called every [`FLUSH_BATCH`] entries while a listing runs and once more
/// when it ends, so:
/// - a listing's children fan out to the other workers mid-listing instead of
///   only after its last `FindNextFile` (a 100k-entry folder no longer holds
///   its own subdirectories back);
/// - the shared store lock is taken once per batch instead of once per
///   discovered subdirectory — the old per-subdirectory registration was the
///   hottest contended lock of the walk;
/// - ids are assigned under the store lock and only then enqueued, so a
///   directory's id always exists before any worker lists it.
///
/// A no-op when nothing is staged or when the scan was cancelled (a cancelled
/// walk publishes nothing and must never grow `active` again).
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn flush_staged(
    engine: &Engine,
    store: &SharedStore,
    queue: &Arc<ParkingMutex<VecDeque<WorkItem>>>,
    work_cv: &Condvar,
    active: &AtomicU32,
    parked: &AtomicU32,
    depth: u32,
    ctx: &mut WorkerCtx,
) {
    if ctx.nodes.is_empty() {
        return;
    }
    if engine.cancel.load(AtomicOrdering::Relaxed) {
        return;
    }

    let (base, files, bytes) = append_to_store(
        &mut ctx.arena,
        &mut ctx.nodes,
        &mut ctx.spans,
        &mut ctx.pairs,
        &mut ctx.top,
        store,
    );
    engine.files.fetch_add(files, AtomicOrdering::Relaxed);
    engine.bytes.fetch_add(bytes, AtomicOrdering::Relaxed);

    if ctx.subdirs.is_empty() {
        return;
    }
    // How many workers to wake for this batch. Each woken worker drains up to
    // POP_BATCH items, so the batch needs `ceil(pushed / POP_BATCH)` of them,
    // and never more than the pool has parked:
    // - one wake (the old empty→non-empty rule) left the whole pool idle
    //   behind a single worker during a wide burst;
    // - blanket notify_all would futex-wake every sleeping thread each time a
    //   deep, narrow branch discovered a single directory, and those threads
    //   would immediately re-park — a syscall per directory for no work done.
    let wake = {
        let mut q = queue.lock();
        let mut pushed = 0usize;
        for staged in ctx.subdirs.drain(..) {
            active.fetch_add(1, AtomicOrdering::Relaxed);
            q.push_back(WorkItem {
                path: staged.path,
                id: base + staged.index,
                depth: depth + 1,
                flags: staged.flags,
            });
            pushed += 1;
        }
        pushed
            .div_ceil(POP_BATCH)
            .min(parked.load(AtomicOrdering::Relaxed) as usize)
    };
    for _ in 0..wake {
        work_cv.notify_one();
    }
}

/// Processes one directory listing on the Windows fast path.
///
/// Per-file work is allocation-free and lock-free: names are copied as UTF-16
/// into the worker's staging arena, classification runs on the find-data
/// buffer with flags inherited from the work item, and sizes come straight out
/// of `WIN32_FIND_DATAW` (no per-file stat anywhere). Per-directory work: one
/// queue lock to hand over the subdirectories discovered so far, and one bulk
/// store append every [`FLUSH_BATCH`] staged entries — both inside
/// [`flush_staged`].
#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn process_dir_fast(
    engine: &Engine,
    store: &SharedStore,
    queue: &Arc<ParkingMutex<VecDeque<WorkItem>>>,
    work_cv: &Condvar,
    active: &AtomicU32,
    parked: &AtomicU32,
    item: WorkItem,
    ctx: &mut WorkerCtx,
) {
    if engine.cancel.load(AtomicOrdering::Relaxed) {
        return;
    }

    // Progress "current" path: shallow directories only (visited first, read
    // every tick) — the lossy conversion is paid for a handful of dirs.
    if item.depth >= 1 && item.depth <= 2 {
        *engine.current.lock() = String::from_utf16_lossy(&item.path);
    }

    // Inherited from the work item: the parent's flags plus this directory's
    // own name, computed once when the parent was listed — the full path is
    // never re-scanned here.
    let flags = item.flags;

    // `dir\*` search pattern straight from the UTF-16 path.
    let mut pattern = Vec::with_capacity(item.path.len() + 2);
    pattern.extend_from_slice(&item.path);
    if pattern.last() != Some(&(b'\\' as u16)) {
        pattern.push(b'\\' as u16);
    }
    pattern.push(b'*' as u16);
    pattern.push(0);

    let mut find_data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
    let handle = unsafe {
        FindFirstFileExW(
            pattern.as_ptr(),
            FindExInfoBasic,
            &mut find_data as *mut _ as *mut _,
            FindExSearchNameMatch,
            std::ptr::null(),
            FIND_FIRST_EX_LARGE_FETCH,
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if err != 2 {
            engine.errors.fetch_add(1, AtomicOrdering::Relaxed);
        }
        return;
    }
    let _find_guard = FindGuard(handle);

    let mut dir_count = 0u64;
    let mut scanned = 0u32;
    loop {
        let name = wide_name(&find_data.cFileName);
        // "." and ".." — plain slice patterns, no string materialization.
        let is_dot = matches!(name, [46] | [46, 46]);
        if !is_dot {
            let attrs = find_data.dwFileAttributes;
            if attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
                let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY != 0;
                if is_dir {
                    // Stage the subdirectory TOGETHER with its node: no store
                    // lock and no id assignment here. The global id
                    // (`base + index`) is handed out by the batch flush below
                    // — still under the store lock — and the work item is only
                    // enqueued afterwards, so a directory's id always exists
                    // in the store before any worker can list it.
                    let local_index = ctx.nodes.len() as u32;
                    let offset = ctx.arena.len() as u32;
                    ctx.arena.extend_from_slice(name);
                    ctx.spans.push((offset, name.len() as u32));
                    ctx.pairs.push((item.id, local_index));
                    ctx.nodes.push(Node {
                        name: String::new(),
                        parent: item.id,
                        is_dir: true,
                        size: 0,
                        file_count: 0,
                        cat: model::CAT_OTHER,
                        kid_count: 0,
                    });
                    // Child search path = parent + '\'; one Vec alloc per
                    // directory (the queue owns it from here on).
                    let mut path = Vec::with_capacity(item.path.len() + 1 + name.len());
                    path.extend_from_slice(&item.path);
                    if path.last() != Some(&(b'\\' as u16)) {
                        path.push(b'\\' as u16);
                    }
                    path.extend_from_slice(name);
                    let child_dir_flags = classify::child_flags(flags, &path, name);
                    ctx.subdirs.push(StagedSubdir {
                        path,
                        index: local_index,
                        flags: child_dir_flags,
                    });
                    dir_count += 1;
                } else {
                    let size =
                        ((find_data.nFileSizeHigh as u64) << 32) | (find_data.nFileSizeLow as u64);
                    let cat = classify::file_category_utf16(name, flags);
                    let offset = ctx.arena.len() as u32;
                    ctx.arena.extend_from_slice(name);
                    ctx.spans.push((offset, name.len() as u32));
                    ctx.pairs.push((item.id, ctx.nodes.len() as u32));
                    ctx.nodes.push(Node {
                        name: String::new(),
                        parent: item.id,
                        is_dir: false,
                        size,
                        file_count: 1,
                        cat,
                        kid_count: 0,
                    });
                }
            }
        }

        let ok = unsafe { FindNextFileW(handle, &mut find_data) };
        if ok == 0 {
            break;
        }
        scanned += 1;
        // Mid-listing handoff: as soon as the batch is full, publish it — the
        // subdirectories found so far reach the other workers while this
        // listing is still running, staging memory stays bounded, and the
        // relaxed cancel load rides along on the same branch (a huge single
        // directory can no longer delay a cancel by its whole listing).
        if scanned & FLUSH_MASK == 0 {
            if engine.cancel.load(AtomicOrdering::Relaxed) {
                break;
            }
            flush_staged(
                engine,
                store,
                queue,
                work_cv,
                active,
                parked,
                item.depth,
                ctx,
            );
        }
    }

    // Tail of the listing: publish when something MUST leave this worker now
    // (a subdirectory to hand over, or a batch that grew past FLUSH_BATCH) —
    // otherwise the staged entries stay local and land in a later flush, the
    // same deferral the old code had for small leaf directories. That keeps
    // the store lock at one per ~FLUSH_BATCH ENTRIES instead of one per
    // directory. After a cancel this is a no-op — exactly like the old code,
    // which dropped staged data outright.
    if !ctx.subdirs.is_empty() || ctx.nodes.len() >= FLUSH_BATCH {
        flush_staged(
            engine,
            store,
            queue,
            work_cv,
            active,
            parked,
            item.depth,
            ctx,
        );
    }
    engine.dirs.fetch_add(dir_count, AtomicOrdering::Relaxed);
}
