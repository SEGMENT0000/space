//! Read-only filesystem walker.
//!
//! Windows uses a pool of workers enumerating directories with
//! `FindFirstFileExW` (`FindExInfoBasic` + `FIND_FIRST_EX_LARGE_FETCH`, UTF-16
//! paths with a verbatim prefix so long paths work). Every entry's size comes
//! from the find data — there is no per-file `stat`. Workers stage a small
//! batch and append it to the shared `TreeBuilder` under one lock per batch.
//!
//! The portable fallback uses `std::fs::read_dir`, one directory at a time.
//! Both paths share classification, cancellation, progress and finalization.
//!
//! Guarantees:
//! - no write/create/rename/delete calls;
//! - directory symlinks, junctions and all other directory reparse points are
//!   skipped (no cycles);
//! - unreadable directories are skipped and counted in `errors`;
//! - progress is emitted by a single coordinator at most every
//!   [`PROGRESS_INTERVAL`] (8/s), followed by exactly one `Done` or `Failed`
//!   from [`spawn_scan`];
//! - cancellation is checked before every directory and every entry.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use crate::engine::classify::{self, DirContext};
use crate::model::{
    CategoryTotals, NodeId, ScanEvent, ScanOutput, ScanProgress, ScanSummary, TreeBuilder,
};

/// Number of largest files retained. Exact by size; ties are ordered by node
/// id so the returned ids are stable and reproducible.
const TOP_FILES: usize = 1000;

/// Coordinator progress cadence: eight reports per second at most.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(125);

pub struct ScanHandle {
    pub cancel: Arc<AtomicBool>,
    pub join: thread::JoinHandle<()>,
}

/// Start a scan on a worker thread.
///
/// The root is validated synchronously: an invalid, non-directory or
/// unreadable root returns `Err` and no thread is spawned. On the worker,
/// exactly one terminal event (`Done` or `Failed`, including cancellation) is
/// sent after any progress events.
pub fn spawn_scan(
    root: PathBuf,
    tx: Sender<ScanEvent>,
    cancel: Arc<AtomicBool>,
) -> io::Result<ScanHandle> {
    prepare_root(&root).map_err(|message| io::Error::new(io::ErrorKind::NotFound, message))?;

    let join_cancel = cancel.clone();
    let join = thread::Builder::new()
        .name("space-scan".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                scan_blocking(root, Some(tx.clone()), join_cancel)
            }));
            let event = match result {
                Ok(Ok(output)) => ScanEvent::Done(Box::new(output)),
                Ok(Err(message)) => ScanEvent::Failed(message),
                Err(_) => ScanEvent::Failed("scan panicked unexpectedly".into()),
            };
            let _ = tx.send(event);
        })?;

    Ok(ScanHandle { cancel, join })
}

/// Scan `root` on the calling thread. Safe to call with no event receiver
/// (headless benchmarks and tests). Returns `Err("Scan cancelled")` when the
/// cancel flag is observed; progress events (when a sender is supplied) carry
/// counters and the directory currently being walked.
pub fn scan_blocking(
    root: PathBuf,
    tx: Option<Sender<ScanEvent>>,
    cancel: Arc<AtomicBool>,
) -> Result<ScanOutput, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Scan cancelled".into());
    }
    let started = Instant::now();
    let (absolute, display) = prepare_root(&root)?;

    #[cfg(windows)]
    {
        fast::scan(absolute, display, tx, cancel, started)
    }
    #[cfg(not(windows))]
    {
        portable::scan(absolute, display, tx, cancel, started)
    }
}

/// Validate the root and resolve it to an absolute path without following a
/// final symlink differently from the walker (metadata follows the link once;
/// inner reparse points are always skipped). The returned string is the
/// user-facing path stored in the tree, without a verbatim prefix.
fn prepare_root(root: &Path) -> Result<(PathBuf, String), String> {
    let absolute = std::path::absolute(root)
        .map_err(|error| format!("cannot resolve {}: {error}", root.display()))?;
    let metadata = std::fs::metadata(&absolute)
        .map_err(|error| format!("cannot access {}: {error}", absolute.display()))?;
    if !metadata.is_dir() {
        return Err(format!("{} is not a folder", absolute.display()));
    }
    // Opening the directory proves it is readable before a worker thread is
    // created, so `spawn_scan` can fail cleanly and synchronously.
    std::fs::read_dir(&absolute)
        .map_err(|error| format!("cannot read {}: {error}", absolute.display()))?;
    let display = display_path(&absolute);
    Ok((absolute, display))
}

fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = text.strip_prefix("\\\\?\\") {
        rest.to_string()
    } else {
        text.into_owned()
    }
}

fn root_label(display: &str) -> String {
    if let Some(name) = Path::new(display).file_name() {
        let name = name.to_string_lossy();
        if !name.is_empty() {
            return name.into_owned();
        }
    }
    let trimmed = display.trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        display.to_string()
    } else {
        trimmed.to_string()
    }
}

/// Keep a per-worker top list bounded. A file can only be in the global top
/// 1000 if it is in its own worker's top 1000, so this is exact by size.
fn retain_top(top: &mut Vec<(u64, NodeId)>) {
    if top.len() > TOP_FILES * 2 {
        sort_top(top);
        top.truncate(TOP_FILES);
    }
}

fn sort_top(top: &mut [(u64, NodeId)]) {
    top.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
}

fn emit_progress(
    tx: &Option<Sender<ScanEvent>>,
    files: u64,
    dirs: u64,
    bytes: u64,
    errors: u64,
    current: String,
    started: Instant,
) {
    if let Some(tx) = tx {
        let _ = tx.send(ScanEvent::Progress(ScanProgress {
            files,
            dirs,
            bytes,
            errors,
            current,
            elapsed_ms: started.elapsed().as_millis() as u64,
        }));
    }
}

fn finish_output(
    builder: TreeBuilder,
    mut largest: Vec<(u64, NodeId)>,
    categories: CategoryTotals,
    dirs: u64,
    errors: u64,
    elapsed_ms: u64,
    root_path: String,
) -> ScanOutput {
    sort_top(&mut largest);
    largest.truncate(TOP_FILES);
    let ids = largest.into_iter().map(|(_, id)| id).collect();
    let tree = builder.finish(ids, categories);
    let root = tree.node(tree.root());
    let total_bytes = root.size;
    let files = root.file_count;
    let summary = ScanSummary {
        root_path,
        total_bytes,
        files,
        dirs,
        errors,
        elapsed_ms,
        categories,
    };
    ScanOutput { tree, summary }
}

// ---------------------------------------------------------------------------
// Windows fast walker: FindFirstFileExW + parallel directory workers.
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod fast {
    use super::*;

    use crate::model::Category;

    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicU64, AtomicUsize};
    use std::sync::mpsc::Sender;

    use parking_lot::{Condvar, Mutex};
    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_FILES, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FIND_FIRST_EX_LARGE_FETCH,
        FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW,
        WIN32_FIND_DATAW,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };

    const FLUSH_BATCH: usize = 512;
    const FLUSH_MASK: usize = FLUSH_BATCH - 1;

    /// How often the coordinator wakes to observe cancellation even when no
    /// worker happens to notify it. Progress is rate-limited separately.
    const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(25);

    struct WorkItem {
        /// Verbatim UTF-16 path (`\\?\C:\...`), no trailing separator unless
        /// it is a volume root.
        path: Vec<u16>,
        id: NodeId,
        depth: u32,
        ctx: DirContext,
        /// True only when `path` is a volume/share root. Used to derive the
        /// root-level Windows / Program Files / ProgramData rules.
        is_drive_root: bool,
    }

    struct Staged {
        name: String,
        parent: NodeId,
        is_dir: bool,
        size: u64,
        cat: Category,
        /// Set for directories: full child path and the child's context.
        subdir: Option<(Vec<u16>, DirContext, u32)>,
    }

    struct WorkerCtx {
        staged: Vec<Staged>,
        staged_dirs: u32,
        top: Vec<(u64, NodeId)>,
        dirs_done: u64,
    }

    impl WorkerCtx {
        fn new() -> Self {
            Self {
                staged: Vec::with_capacity(FLUSH_BATCH),
                staged_dirs: 0,
                top: Vec::with_capacity(TOP_FILES + 16),
                dirs_done: 0,
            }
        }
    }

    struct BuilderState {
        builder: Option<TreeBuilder>,
        categories: CategoryTotals,
    }

    struct Shared {
        state: Mutex<BuilderState>,
        queue: Mutex<VecDeque<WorkItem>>,
        cv: Condvar,
        active: AtomicUsize,
        cancel: Arc<AtomicBool>,
        files: AtomicU64,
        dirs: AtomicU64,
        bytes: AtomicU64,
        errors: AtomicU64,
        current: Mutex<String>,
        fatal: Mutex<Option<String>>,
        root_display: String,
    }

    struct FindGuard(HANDLE);

    impl Drop for FindGuard {
        fn drop(&mut self) {
            unsafe {
                FindClose(self.0);
            }
        }
    }

    pub(super) fn scan(
        absolute: PathBuf,
        display: String,
        tx: Option<Sender<ScanEvent>>,
        cancel: Arc<AtomicBool>,
        started: Instant,
    ) -> Result<ScanOutput, String> {
        let root_ctx = DirContext::from_path(&absolute);
        let root_is_drive = classify::is_drive_root(&absolute);
        let label = root_label(&display);
        let builder = TreeBuilder::new(display.clone(), label);
        let shared = Arc::new(Shared {
            state: Mutex::new(BuilderState {
                builder: Some(builder),
                categories: CategoryTotals::default(),
            }),
            queue: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
            active: AtomicUsize::new(1),
            cancel: cancel.clone(),
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            current: Mutex::new(display.clone()),
            fatal: Mutex::new(None),
            root_display: display.clone(),
        });

        shared.queue.lock().push_back(WorkItem {
            path: wide_absolute(&absolute),
            id: 0,
            depth: 0,
            ctx: root_ctx,
            is_drive_root: root_is_drive,
        });

        let worker_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 16);
        let mut handles = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let worker_shared = Arc::clone(&shared);
            match thread::Builder::new()
                .name(format!("space-walk-{index}"))
                .spawn(move || worker(worker_shared))
            {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    if handles.is_empty() {
                        return Err(format!("cannot start scan worker: {error}"));
                    }
                    break;
                }
            }
        }

        let mut last_progress = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                // Drop queued work instead of walking it; in-flight workers
                // observe the flag before their next entry. The queue is
                // replaced under the lock and dropped outside it.
                let drained = {
                    let mut queue = shared.queue.lock();
                    std::mem::take(&mut *queue)
                };
                let queued = drained.len();
                if queued > 0 {
                    shared.active.fetch_sub(queued, Ordering::AcqRel);
                }
                drop(drained);
                {
                    let _queue = shared.queue.lock();
                    shared.cv.notify_all();
                }
                break;
            }

            {
                let mut queue = shared.queue.lock();
                if shared.active.load(Ordering::Acquire) == 0 && queue.is_empty() {
                    break;
                }
                let wait = PROGRESS_INTERVAL
                    .saturating_sub(last_progress.elapsed())
                    .min(CANCEL_POLL_INTERVAL);
                if wait.is_zero() {
                    drop(queue);
                    emit_progress(
                        &tx,
                        shared.files.load(Ordering::Relaxed),
                        shared.dirs.load(Ordering::Relaxed),
                        shared.bytes.load(Ordering::Relaxed),
                        shared.errors.load(Ordering::Relaxed),
                        shared.current.lock().clone(),
                        started,
                    );
                    last_progress = Instant::now();
                    continue;
                }
                shared.cv.wait_for(&mut queue, wait);
            }

            if last_progress.elapsed() >= PROGRESS_INTERVAL {
                emit_progress(
                    &tx,
                    shared.files.load(Ordering::Relaxed),
                    shared.dirs.load(Ordering::Relaxed),
                    shared.bytes.load(Ordering::Relaxed),
                    shared.errors.load(Ordering::Relaxed),
                    shared.current.lock().clone(),
                    started,
                );
                last_progress = Instant::now();
            }
        }

        let mut tops = Vec::new();
        let mut worker_panicked = false;
        for handle in handles {
            match handle.join() {
                Ok(top) => tops.extend(top),
                Err(_) => worker_panicked = true,
            }
        }

        if cancel.load(Ordering::Relaxed) {
            return Err("Scan cancelled".into());
        }
        if let Some(message) = shared.fatal.lock().clone() {
            return Err(message);
        }
        if worker_panicked {
            return Err("a scan worker panicked".into());
        }

        let dirs = shared.dirs.load(Ordering::Relaxed);
        let errors = shared.errors.load(Ordering::Relaxed);
        let (builder, categories) = {
            let mut state = shared.state.lock();
            let builder = state
                .builder
                .take()
                .ok_or_else(|| "internal error: scan tree already finalized".to_string())?;
            (builder, state.categories)
        };

        Ok(finish_output(
            builder,
            tops,
            categories,
            dirs,
            errors,
            started.elapsed().as_millis() as u64,
            display,
        ))
    }

    fn worker(shared: Arc<Shared>) -> Vec<(u64, NodeId)> {
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
        let mut ctx = WorkerCtx::new();

        loop {
            if shared.cancel.load(Ordering::Relaxed) {
                wake_all(&shared);
                break;
            }

            let item = {
                let mut queue = shared.queue.lock();
                loop {
                    if let Some(item) = queue.pop_front() {
                        break Some(item);
                    }
                    if shared.active.load(Ordering::Acquire) == 0 {
                        break None;
                    }
                    shared.cv.wait(&mut queue);
                    if shared.cancel.load(Ordering::Relaxed) {
                        break None;
                    }
                }
            };

            let Some(item) = item else {
                break;
            };

            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                process_dir(&shared, &item, &mut ctx);
            }));
            if caught.is_err() {
                // A panic in one directory must not strand the completion
                // counter, but a partially built level must not be reported as
                // a successful exact scan either. Fail the scan after the
                // other workers have stopped.
                ctx.staged.clear();
                ctx.staged_dirs = 0;
                let mut fatal = shared.fatal.lock();
                if fatal.is_none() {
                    *fatal = Some("a scan worker panicked".into());
                }
            }

            if shared.active.fetch_sub(1, Ordering::AcqRel) == 1 {
                let _queue = shared.queue.lock();
                shared.cv.notify_all();
            }
        }

        if !shared.cancel.load(Ordering::Relaxed) {
            // Any discovered directory is always flushed before its work item
            // retires, so this tail can only contain files.
            flush(&shared, &mut ctx);
        }
        sort_top(&mut ctx.top);
        ctx.top.truncate(TOP_FILES);
        ctx.top
    }

    fn wake_all(shared: &Shared) {
        let _queue = shared.queue.lock();
        shared.cv.notify_all();
    }

    fn process_dir(shared: &Shared, item: &WorkItem, ctx: &mut WorkerCtx) {
        if shared.cancel.load(Ordering::Relaxed) {
            wake_all(shared);
            return;
        }

        // Updating the readout for every deep directory would allocate and
        // contend for a string nobody can read. Shallow directories are
        // visited first; the periodic sample keeps it moving in deep trees.
        if item.depth <= 3 || ctx.dirs_done.is_multiple_of(64) {
            *shared.current.lock() = display_from_wide(&item.path);
        }

        let pattern = pattern_for(&item.path);
        let mut find_data = WIN32_FIND_DATAW::default();
        let handle = unsafe {
            FindFirstFileExW(
                pattern.as_ptr(),
                FindExInfoBasic,
                &mut find_data as *mut _ as *mut c_void,
                FindExSearchNameMatch,
                std::ptr::null(),
                FIND_FIRST_EX_LARGE_FETCH,
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            let error = unsafe { GetLastError() };
            if error == ERROR_FILE_NOT_FOUND || error == ERROR_NO_MORE_FILES {
                // Empty directory, or it vanished between discovery and this
                // listing. Not a failure.
                ctx.dirs_done = ctx.dirs_done.saturating_add(1);
                return;
            }
            if item.depth == 0 {
                *shared.fatal.lock() = Some(format!(
                    "cannot read {}: {}",
                    shared.root_display,
                    io::Error::from_raw_os_error(error as i32)
                ));
            } else {
                shared.errors.fetch_add(1, Ordering::Relaxed);
            }
            ctx.dirs_done = ctx.dirs_done.saturating_add(1);
            return;
        }
        let _find = FindGuard(handle);

        let mut seen = 0usize;
        loop {
            if shared.cancel.load(Ordering::Relaxed) {
                wake_all(shared);
                return;
            }

            let name_wide = wide_name(&find_data.cFileName);
            if !is_dot(name_wide) {
                let attributes = find_data.dwFileAttributes;
                let is_dir = attributes & FILE_ATTRIBUTE_DIRECTORY != 0;
                let is_reparse = attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0;

                // Never descend into junctions/symlinks/mount points. File
                // reparse points (e.g. cloud placeholders) are counted as
                // files at their reported size; they are never followed.
                if !(is_dir && is_reparse) {
                    let name = String::from_utf16_lossy(name_wide);
                    if !name.is_empty() {
                        if is_dir {
                            let child_ctx = item.ctx.child(&name, item.is_drive_root);
                            let category = classify::classify_with_ctx(&name, child_ctx, true);
                            let mut path =
                                Vec::with_capacity(item.path.len() + 1 + name_wide.len());
                            path.extend_from_slice(&item.path);
                            if path.last() != Some(&(b'\\' as u16)) {
                                path.push(b'\\' as u16);
                            }
                            path.extend_from_slice(name_wide);
                            ctx.staged.push(Staged {
                                name,
                                parent: item.id,
                                is_dir: true,
                                size: 0,
                                cat: category,
                                subdir: Some((path, child_ctx, item.depth + 1)),
                            });
                            ctx.staged_dirs = ctx.staged_dirs.saturating_add(1);
                        } else {
                            let size = ((find_data.nFileSizeHigh as u64) << 32)
                                | find_data.nFileSizeLow as u64;
                            let category = classify::classify_with_ctx(&name, item.ctx, false);
                            ctx.staged.push(Staged {
                                name,
                                parent: item.id,
                                is_dir: false,
                                size,
                                cat: category,
                                subdir: None,
                            });
                        }
                    }
                }
            }

            let more = unsafe { FindNextFileW(handle, &mut find_data) };
            if more == 0 {
                let error = unsafe { GetLastError() };
                if error != ERROR_NO_MORE_FILES {
                    shared.errors.fetch_add(1, Ordering::Relaxed);
                }
                break;
            }

            seen += 1;
            if seen & FLUSH_MASK == 0 {
                flush(shared, ctx);
            }
        }

        ctx.dirs_done = ctx.dirs_done.saturating_add(1);
        // A discovered directory must reach the queue before this work item
        // retires; otherwise a worker could exit while the only reference to
        // that subtree is still local staging.
        if ctx.staged_dirs > 0 || ctx.staged.len() >= FLUSH_BATCH {
            flush(shared, ctx);
        }
    }

    fn flush(shared: &Shared, ctx: &mut WorkerCtx) {
        if ctx.staged.is_empty() {
            ctx.staged_dirs = 0;
            return;
        }
        if shared.cancel.load(Ordering::Relaxed) {
            ctx.staged.clear();
            ctx.staged_dirs = 0;
            return;
        }

        let mut files = 0u64;
        let mut dirs = 0u64;
        let mut bytes = 0u64;
        let mut children: Vec<(NodeId, Vec<u16>, DirContext, u32)> = Vec::new();

        {
            let mut state = shared.state.lock();
            let BuilderState {
                builder,
                categories,
            } = &mut *state;
            let builder = builder
                .as_mut()
                .expect("scan builder exists until finalize");
            for entry in ctx.staged.drain(..) {
                let id = builder.add_node(
                    &entry.name,
                    entry.parent,
                    entry.is_dir,
                    entry.size,
                    entry.cat,
                );
                if entry.is_dir {
                    dirs += 1;
                    if let Some((path, child_ctx, depth)) = entry.subdir {
                        children.push((id, path, child_ctx, depth));
                    }
                } else {
                    files += 1;
                    bytes += entry.size;
                    categories.add(entry.cat, entry.size);
                    ctx.top.push((entry.size, id));
                }
            }
        }

        retain_top(&mut ctx.top);
        if files > 0 {
            shared.files.fetch_add(files, Ordering::Relaxed);
            shared.bytes.fetch_add(bytes, Ordering::Relaxed);
        }
        if dirs > 0 {
            shared.dirs.fetch_add(dirs, Ordering::Relaxed);
        }
        ctx.staged_dirs = 0;

        if !children.is_empty() {
            if shared.cancel.load(Ordering::Relaxed) {
                return;
            }
            shared.active.fetch_add(children.len(), Ordering::AcqRel);
            {
                let mut queue = shared.queue.lock();
                for (id, path, ctx, depth) in children {
                    queue.push_back(WorkItem {
                        path,
                        id,
                        depth,
                        ctx,
                        is_drive_root: false,
                    });
                }
                shared.cv.notify_all();
            }
        }
    }

    /// Absolute path as a NUL-free, backslash-normalized verbatim UTF-16 path.
    /// `\\?\` lifts the MAX_PATH limit for the raw Win32 calls.
    fn wide_absolute(path: &Path) -> Vec<u16> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        for unit in &mut wide {
            if *unit == b'/' as u16 {
                *unit = b'\\' as u16;
            }
        }
        if wide.starts_with(&[92, 92, 63, 92]) {
            return wide;
        }
        if wide.starts_with(&[92, 92]) {
            let mut verbatim = "\\\\?\\UNC\\".encode_utf16().collect::<Vec<_>>();
            verbatim.extend_from_slice(&wide[2..]);
            verbatim
        } else {
            let mut verbatim = "\\\\?\\".encode_utf16().collect::<Vec<_>>();
            verbatim.extend_from_slice(&wide);
            verbatim
        }
    }

    fn pattern_for(path: &[u16]) -> Vec<u16> {
        let mut pattern = Vec::with_capacity(path.len() + 2);
        pattern.extend_from_slice(path);
        if pattern.last() != Some(&(b'\\' as u16)) {
            pattern.push(b'\\' as u16);
        }
        pattern.push(b'*' as u16);
        pattern.push(0);
        pattern
    }

    fn wide_name(buffer: &[u16; 260]) -> &[u16] {
        let len = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        &buffer[..len]
    }

    fn is_dot(name: &[u16]) -> bool {
        matches!(name, [46] | [46, 46])
    }

    fn display_from_wide(path: &[u16]) -> String {
        let text = String::from_utf16_lossy(path);
        if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
            format!("\\\\{rest}")
        } else if let Some(rest) = text.strip_prefix("\\\\?\\") {
            rest.to_string()
        } else {
            text
        }
    }
}

// ---------------------------------------------------------------------------
// Portable fallback: std::fs, one directory at a time.
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
mod portable {
    use super::*;

    use std::sync::mpsc::Sender;

    pub(super) fn scan(
        absolute: PathBuf,
        display: String,
        tx: Option<Sender<ScanEvent>>,
        cancel: Arc<AtomicBool>,
        started: Instant,
    ) -> Result<ScanOutput, String> {
        if let Err(error) = std::fs::read_dir(&absolute) {
            return Err(format!("cannot read {}: {error}", absolute.display()));
        }

        let root_ctx = DirContext::from_path(&absolute);
        let root_is_drive = classify::is_drive_root(&absolute);
        let mut builder = TreeBuilder::new(display.clone(), root_label(&display));
        let mut categories = CategoryTotals::default();
        let mut files = 0u64;
        let mut dirs = 0u64;
        let mut bytes = 0u64;
        let mut errors = 0u64;
        let mut largest: Vec<(u64, NodeId)> = Vec::new();
        let mut last_report = Instant::now();

        // (path, node id, inherited context, is volume root)
        let mut stack: Vec<(PathBuf, NodeId, DirContext, bool)> =
            vec![(absolute, 0, root_ctx, root_is_drive)];

        while let Some((dir_path, dir_id, ctx, is_drive_root)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return Err("Scan cancelled".into());
            }

            let read = match std::fs::read_dir(&dir_path) {
                Ok(read) => read,
                Err(_) => {
                    if dir_id == 0 {
                        return Err(format!("cannot read {}", dir_path.display()));
                    }
                    errors = errors.saturating_add(1);
                    continue;
                }
            };

            for entry in read {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Scan cancelled".into());
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => {
                        errors = errors.saturating_add(1);
                        continue;
                    }
                };
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.is_empty() {
                    continue;
                }
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(_) => {
                        errors = errors.saturating_add(1);
                        continue;
                    }
                };
                // Never follow directory symlinks. The portable test/bench
                // path has no Windows reparse-point concept beyond symlinks.
                if file_type.is_symlink() {
                    continue;
                }

                if file_type.is_dir() {
                    let child_ctx = ctx.child(&name, is_drive_root);
                    let category = classify::classify_with_ctx(&name, child_ctx, true);
                    let id = builder.add_node(&name, dir_id, true, 0, category);
                    dirs = dirs.saturating_add(1);
                    stack.push((entry.path(), id, child_ctx, false));
                } else if file_type.is_file() {
                    let metadata = match entry.metadata() {
                        Ok(metadata) => metadata,
                        Err(_) => {
                            errors = errors.saturating_add(1);
                            continue;
                        }
                    };
                    let size = metadata.len();
                    let category = classify::classify_with_ctx(&name, ctx, false);
                    let id = builder.add_node(&name, dir_id, false, size, category);
                    files = files.saturating_add(1);
                    bytes = bytes.saturating_add(size);
                    categories.add(category, size);
                    largest.push((size, id));
                    retain_top(&mut largest);
                }

                if last_report.elapsed() >= PROGRESS_INTERVAL {
                    emit_progress(
                        &tx,
                        files,
                        dirs,
                        bytes,
                        errors,
                        dir_path.to_string_lossy().into_owned(),
                        started,
                    );
                    last_report = Instant::now();
                }
            }
        }

        Ok(finish_output(
            builder,
            largest,
            categories,
            dirs,
            errors,
            started.elapsed().as_millis() as u64,
            display,
        ))
    }
}
