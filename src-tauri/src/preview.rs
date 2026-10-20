//! `get_preview` — family-sniffing file/folder previews (FLEET_SPEC §A/§B).
//!
//! One command, one payload: the module sniffs dir-vs-file and the preview
//! family itself, enforces every size cap BEFORE decoding, and caches results
//! in a managed LRU keyed `(path, mtime, len)`. A fallback ladder (richest
//! family → text head → hex head → magic/signature card) keeps every readable
//! regular file on a REAL card, and every rejection but an absent path
//! degrades to a magic card carrying the reason verbatim — `Err` survives
//! only for an empty path or "not found". Errors are lowercase and specific
//! (`"not found"`, `"access denied"`, `"file in use"`) — the note row is
//! already the card's failed-read line, so short reasons beat a repeated
//! `unreadable:` prefix that clips inside a narrow card.

use std::fs::{self, File};
use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::UNIX_EPOCH;

use crate::model::{
    DbPreview, DbTable, HexPreview, HexRow, ImagePreview, ListEntry, ListPreview, MediaPreview,
    MetaPair, MetaPreview, PreviewPayload, TextPreview,
};

// --- Family caps (FLEET_SPEC §A/§B). Enforced BEFORE any decode. ---
const TEXT_CAP: u64 = 64 * 1024; // TEXT/SVG head read
const TEXT_LINES: usize = 120; // returned line cap
const HEX_CAP: u64 = 4 * 1024; // hexdump head
const LIST_CAP: usize = 200; // list entry cap (folders, archives, tar, sheets)
const DIR_HEAD: usize = 1024; // folder entries that get metadata (size sort head)
const DIR_COUNT_MAX: u64 = 50_000; // exact folder totals stop being "cheap" here
const IMAGE_BYTES_CAP: u64 = 32 * 1024 * 1024; // decode input cap (oversize → meta)
const IMAGE_MP: u64 = 8_000_000; // 8 megapixel decode cap
const IMAGE_EDGE: u32 = 512; // thumbnail longest edge
const IMAGE_JPEG_Q: u8 = 80; // thumbnail quality
const DB_TABLES_CAP: usize = 100; // table list cap (never COUNT(*))
const DB_ROWS_TABLES: usize = 3; // leading tables that get sampled rows
const DB_ROWS: usize = 5; // sampled rows per table (bounded LIMIT)
const DB_CELL: usize = 64; // display chars per sampled cell
const META_HEAD: u64 = 64 * 1024; // signature/parser head read
const PDF_SCAN: u64 = 1024 * 1024; // bounded page-count scan for PDFs
const TABLE_READ_CAP: u64 = 16 * 1024; // targeted table reads (fonts, PE/ELF tails)
const CACHE_CAP: usize = 256; // managed LRU entries
const SEM_PERMITS: u8 = 4; // concurrent heavy previews

// ---------------------------------------------------------------------------
// Managed state: LRU cache + async concurrency gate.
// ---------------------------------------------------------------------------

/// Keyed `(path, mtime, len)` per §B — any file change misses the cache.
#[derive(PartialEq, Eq)]
struct CacheKey {
    path: String,
    mtime_ms: u64,
    len: u64,
}

/// Hand-rolled LRU (self-contained; mirrors the vector pattern in scanner.rs
/// but cannot import scanner internals). Most-recent entries live at the back.
struct LruCache {
    entries: Vec<(CacheKey, PreviewPayload)>,
    cap: usize,
}

impl LruCache {
    fn new(cap: usize) -> Self {
        Self {
            entries: Vec::with_capacity(cap),
            cap,
        }
    }

    fn get(&mut self, key: &CacheKey) -> Option<PreviewPayload> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        let (k, v) = self.entries.remove(index);
        let payload = v.clone();
        self.entries.push((k, v));
        Some(payload)
    }

    fn insert(&mut self, key: CacheKey, value: PreviewPayload) {
        while self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        self.entries.push((key, value));
    }
}

/// Everything `get_preview` manages: the LRU + the ≤4-permit gate.
pub struct PreviewState {
    cache: Mutex<LruCache>,
    sem: Arc<Semaphore>,
}

impl PreviewState {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(LruCache::new(CACHE_CAP)),
            sem: Arc::new(Semaphore::new(SEM_PERMITS)),
        }
    }
}

/// Async semaphore (waker list) — no extra crates allowed (§B lists only
/// image/zip/rusqlite/base64), so tokio's semaphore is unavailable and a
/// std condvar would block runtime workers while I/O runs.
struct Semaphore {
    state: Mutex<SemState>,
}

struct SemState {
    permits: u8,
    waiters: Vec<Waker>,
}

struct PermitFuture {
    sem: Arc<Semaphore>,
}

/// Held for the duration of a payload build; `Drop` hands the permit on.
struct Permit {
    sem: Arc<Semaphore>,
}

fn lock_sem<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A poisoned semaphore/cache lock only means a prior panic; the data is
    // still internally consistent enough to keep serving previews.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Semaphore {
    fn new(permits: u8) -> Self {
        Self {
            state: Mutex::new(SemState {
                permits,
                waiters: Vec::new(),
            }),
        }
    }
}

impl Future for PermitFuture {
    type Output = Permit;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Permit> {
        let mut state = lock_sem(&self.sem.state);
        if state.permits > 0 {
            state.permits -= 1;
            return Poll::Ready(Permit {
                sem: self.sem.clone(),
            });
        }
        if !state.waiters.iter().any(|w| w.will_wake(cx.waker())) {
            state.waiters.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let waker = {
            let mut state = lock_sem(&self.sem.state);
            state.permits += 1;
            state.waiters.pop()
        };
        // Wake one taker; if a newcomer steals the permit first, the woken
        // task re-registers — no lost wakeups (release always bumps permits).
        if let Some(w) = waker {
            w.wake();
        }
    }
}

fn acquire(sem: &Arc<Semaphore>) -> PermitFuture {
    PermitFuture {
        sem: sem.clone(),
    }
}

// ---------------------------------------------------------------------------
// Entry point.
// ---------------------------------------------------------------------------

/// Content preview for a file OR folder — sniffs dir-vs-file and the family
/// internally. Heavy work runs under the ≤4-permit gate on the async runtime.
///
/// Rejection policy: `Err` ONLY for an empty path or a genuinely absent file
/// ("not found"). Every other failure — symlink, non-regular, ACL-denied,
/// exclusively locked, vanished mid-read — degrades to a magic card carrying
/// the reason verbatim in its `note` pair, so a tile for a file that exists
/// never renders as "no preview".
pub async fn get_preview(state: &PreviewState, path: String) -> Result<PreviewPayload, String> {
    if path.is_empty() {
        return Err("not found".to_string());
    }
    // Safety (§B): identify symlinks up front (the walker's skip rule — we
    // never follow a symlink target), then canonicalize for a stable identity.
    let pre = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("not found".to_string());
        }
        Err(e) => return Ok(degraded_meta(Path::new(&path), &io_err(e), false)),
    };
    if pre.file_type().is_symlink() {
        // Never read through a link — the link's own stat, no signature.
        return Ok(degraded_meta(Path::new(&path), "symlink", false));
    }
    let canon = match fs::canonicalize(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("not found".to_string());
        }
        Err(e) => return Ok(degraded_meta(Path::new(&path), &io_err(e), false)),
    };
    let meta = match fs::metadata(&canon) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("not found".to_string());
        }
        Err(e) => return Ok(degraded_meta(&canon, &io_err(e), false)),
    };
    let key = CacheKey {
        path: canon.to_string_lossy().into_owned(),
        mtime_ms: mtime_ms(&meta),
        len: if meta.is_dir() { 0 } else { meta.len() },
    };

    // Cache lookup happens before the gate so repeat views cost nothing.
    if let Some(payload) = lock_sem(&state.cache).get(&key) {
        return Ok(payload);
    }

    let _permit = acquire(&state.sem).await;
    // Double-check: another task may have filled the entry while we waited.
    if let Some(payload) = lock_sem(&state.cache).get(&key) {
        return Ok(payload);
    }

    // Degraded cards are deliberately NOT cached: they describe a transient
    // access state, and their (mtime, len) key may be unknown anyway.
    let payload = if meta.is_dir() {
        match build_dir_list(&canon, &meta) {
            Ok(p) => p,
            Err(e) if e == "not found" => return Err(e),
            Err(e) => return Ok(degraded_meta(&canon, &e, false)),
        }
    } else if meta.is_file() {
        match build_file(&canon, &meta) {
            Ok(p) => p,
            Err(e) if e == "not found" => return Err(e),
            Err(e) => return Ok(degraded_meta(&canon, &e, false)),
        }
    } else {
        // Sockets/pipes/devices — non-regular: never followed, never read.
        return Ok(degraded_meta(&canon, "unsupported", false));
    };

    lock_sem(&state.cache).insert(key, payload.clone());
    Ok(payload)
}

/// Best-effort magic card for a path that cannot be fully previewed. Every
/// pair is real: signature bytes only when reading is proven safe (`follow` —
/// never through a link), mtime/size when stat-able — plus the rejection
/// reason verbatim in `note`. Nothing unknown is ever printed.
fn degraded_meta(path: &Path, reason: &str, follow: bool) -> PreviewPayload {
    let stat = if follow {
        fs::metadata(path).ok()
    } else {
        fs::symlink_metadata(path).ok()
    };
    // A directory's file-extension says nothing about its contents, and a
    // directory's own byte size is not its content size — both suppressed.
    let (len, modified) = match &stat {
        Some(m) if !m.is_dir() => (
            Some(m.len()),
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok()),
        ),
        _ => (None, None),
    };
    let ext = if stat.as_ref().is_some_and(|m| m.is_dir()) {
        String::new()
    } else {
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default()
    };
    let head = if follow {
        read_head(path, len.unwrap_or(META_HEAD).min(META_HEAD)).unwrap_or_default()
    } else {
        Vec::new()
    };
    let mut pairs = vec![pair("note", reason.to_string())];
    if let Some(name) = sig_format(&head) {
        pairs.push(pair("format", name));
    }
    if let Some(d) = modified {
        pairs.push(pair("modified", fmt_unix_utc(d.as_secs())));
    }
    meta_payload(pairs, &head, len, &ext)
}

/// Lowercase, specific io messages per the shared error convention.
fn io_err(e: std::io::Error) -> String {
    // Windows system files fail with sharing/lock violations, and `io::Error`'s
    // Display for those is a full OS sentence ("the process cannot access the
    // file because it is being used by another process. (os error 32)") — it
    // gets clipped mid-word inside a 260px plate head. The raw code is the only
    // reliable signal here: ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION both
    // mean "someone else has it open", which is the honest short genre.
    #[cfg(windows)]
    if let Some(code) = e.raw_os_error() {
        match code {
            32 | 33 => return "file in use".to_string(),
            5 => return "access denied".to_string(),
            _ => {}
        }
    }
    match e.kind() {
        std::io::ErrorKind::NotFound => "not found".to_string(),
        std::io::ErrorKind::PermissionDenied => "access denied".to_string(),
        _ => unreadable(e),
    }
}

/// Generic lowercase failure text (`unreadable: <reason>`) for zip and
/// rusqlite errors. One helper instead of a dozen copies of the same closure.
fn unreadable(e: impl std::fmt::Display) -> String {
    format!("unreadable: {}", e.to_string().to_lowercase())
}

fn mtime_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Bounded reads — never the whole file.
// ---------------------------------------------------------------------------

/// Reads at most `n` bytes from the start (stops cleanly at EOF).
fn read_head(path: &Path, n: u64) -> Result<Vec<u8>, String> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path).map_err(io_err)?;
    read_up_to(&mut file, n)
}

/// Reads at most `n` bytes at `off` (targeted parser reads: PE tail, fonts…).
fn read_at(path: &Path, off: u64, n: u64) -> Result<Vec<u8>, String> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path).map_err(io_err)?;
    file.seek(SeekFrom::Start(off)).map_err(io_err)?;
    read_up_to(&mut file, n)
}

fn read_up_to(file: &mut File, n: u64) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n as usize];
    let mut filled = 0usize;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(k) => filled += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_err(e)),
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Formatting helpers for META pairs (every value = parsed bytes).
// ---------------------------------------------------------------------------

fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Lowercase space-separated hex — same byte style as the HEX family.
fn hex_str(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Howard Hinnant's civil_from_days — no chrono crate is allowed (§B).
fn fmt_unix_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        rem / 3600,
        (rem % 3600) / 60
    )
}

fn pair(k: &str, v: String) -> MetaPair {
    MetaPair {
        k: k.to_string(),
        v,
    }
}

/// All-blocks-`None` payload shell — each family constructor fills its one
/// block. WHY: seven constructors repeated the same 8-field literal; this is
/// the single source of that shape (fields/staying-in-sync with model.rs).
fn payload_shell(family: &str) -> PreviewPayload {
    PreviewPayload {
        family: family.to_string(),
        text: None,
        image: None,
        hex: None,
        list: None,
        meta: None,
        db: None,
        media: None,
    }
}

/// Magic/signature card (family "meta"): parsed pairs lead, then the honest
/// extension type label, the raw signature bytes, and the size — `len` is
/// `None` when the file could not be stat'ed (nothing unknown is printed).
/// Content detection comes ONLY from bytes; the extension only labels itself.
fn meta_payload(
    mut pairs: Vec<MetaPair>,
    head: &[u8],
    len: Option<u64>,
    ext: &str,
) -> PreviewPayload {
    let label = ext_class(ext);
    pairs.push(pair(
        "type",
        if ext.is_empty() {
            label.to_string()
        } else {
            format!("{label} (.{ext})")
        },
    ));
    if !head.is_empty() {
        let n = head.len().min(8);
        pairs.push(pair("signature", hex_str(&head[..n])));
    }
    if let Some(len) = len {
        pairs.push(pair("size", fmt_bytes(len)));
    }
    let mut payload = payload_shell("meta");
    payload.meta = Some(MetaPreview { pairs });
    payload
}

// ---------------------------------------------------------------------------
// TEXT family (also SVG — integrator note 2: sanitized source lines).
// ---------------------------------------------------------------------------

enum TextClass {
    Utf8,
    Utf16Le,
    Utf16Be,
    Binary,
}

/// Content sniff used by the TEXT family and by unknown-extension routing.
/// Handles BOMs (UTF-8/16), BOM-less UTF-16 heuristics, and a printable-ratio
/// check so binary blobs never render as garbage text.
fn classify_text(head: &[u8]) -> TextClass {
    if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return TextClass::Utf8;
    }
    if head.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) || head.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
    {
        return TextClass::Binary; // UTF-32 BOM — not worth a lossy guess.
    }
    if head.starts_with(&[0xFF, 0xFE]) {
        return TextClass::Utf16Le;
    }
    if head.starts_with(&[0xFE, 0xFF]) {
        return TextClass::Utf16Be;
    }

    let sample = &head[..head.len().min(512)];
    if sample.contains(&0) {
        // BOM-less UTF-16: NULs on one side of every other byte, none on the other.
        let half = sample.len() / 2;
        let mut even_zeros = 0usize;
        let mut odd_zeros = 0usize;
        for (i, b) in sample.iter().enumerate() {
            if *b == 0 {
                if i % 2 == 0 {
                    even_zeros += 1;
                } else {
                    odd_zeros += 1;
                }
            }
        }
        if odd_zeros * 10 >= half * 3 && even_zeros == 0 && half >= 8 {
            return TextClass::Utf16Le;
        }
        if even_zeros * 10 >= half * 3 && odd_zeros == 0 && half >= 8 {
            return TextClass::Utf16Be;
        }
        return TextClass::Binary;
    }

    let mut bad = 0usize;
    for &b in sample {
        let ok = b == b'\t' || b == b'\n' || b == b'\r' || (0x20..0x7F).contains(&b) || b >= 0x80;
        if !ok {
            bad += 1;
        }
    }
    // A few stray control bytes (ESC in ANSI logs) are fine; the rest aren't.
    if bad.saturating_mul(20) > sample.len() {
        return TextClass::Binary;
    }
    TextClass::Utf8
}

fn decode_utf16(bytes: &[u8], little: bool) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| {
            if little {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect();
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Strips C0 control chars (except tab) — keeps a plate render safe.
fn scrub_line(line: &str) -> String {
    if line.chars().all(|c| c >= ' ' || c == '\t') {
        return line.to_string();
    }
    line.chars()
        .filter(|c| *c >= ' ' || *c == '\t')
        .collect()
}

/// SVG (integrator note 2): additionally drop `<script>` bodies — minified
/// single-line SVGs carry the whole payload on one line, so track state
/// across lines too.
fn scrub_svg_line(line: &str, in_script: &mut bool) -> String {
    let scrubbed = scrub_line(line);
    let lower = scrubbed.to_ascii_lowercase();
    if *in_script {
        return match lower.find("</script>") {
            Some(end) => {
                *in_script = false;
                let rest = &scrubbed[end + 9..];
                if rest.trim().is_empty() {
                    String::new()
                } else {
                    scrub_svg_line(rest, in_script)
                }
            }
            None => String::new(),
        };
    }
    match lower.find("<script") {
        Some(start) => match lower[start..].find('>') {
            // '<script …>' opener with no '>' yet — wait for a later line.
            None => {
                *in_script = true;
                scrubbed[..start].to_string()
            }
            Some(gt) => {
                let after = start + gt + 1;
                let close = lower[after..].find("</script>");
                match close {
                    None => {
                        *in_script = true;
                        scrubbed[..after].to_string()
                    }
                    Some(c) => {
                        let end = after + c;
                        let keep = format!("{}{}", &scrubbed[..after], &scrubbed[end + 9..]);
                        // Recurse: another script may follow on the same line.
                        scrub_svg_line(&keep, in_script)
                    }
                }
            }
        },
        None => scrubbed,
    }
}

/// Reads the TEXT head and decodes it. `Ok(None)` means the content is binary
/// — the caller falls back to HEX instead of rendering garbage.
fn build_text(path: &Path, len: u64, is_svg: bool) -> Result<Option<TextPreview>, String> {
    let raw = read_head(path, len.min(TEXT_CAP))?;
    Ok(text_from_head(&raw, len, is_svg))
}

/// Builds the TEXT block from an already-read head. Shared with the fallback
/// ladder, which reuses its 64 KB head for the text rung.
fn text_from_head(raw: &[u8], len: u64, is_svg: bool) -> Option<TextPreview> {
    let class = match classify_text(raw) {
        TextClass::Binary => return None,
        c => c,
    };

    let body: String = match class {
        TextClass::Utf8 => {
            let start = usize::from(raw.starts_with(&[0xEF, 0xBB, 0xBF]));
            String::from_utf8_lossy(&raw[start..]).into_owned()
        }
        TextClass::Utf16Le => {
            let start = if raw.starts_with(&[0xFF, 0xFE]) { 2 } else { 0 };
            decode_utf16(&raw[start..], true)
        }
        TextClass::Utf16Be => {
            let start = if raw.starts_with(&[0xFE, 0xFF]) { 2 } else { 0 };
            decode_utf16(&raw[start..], false)
        }
        TextClass::Binary => unreachable!(),
    };

    let whole_file = raw.len() as u64 == len; // totalLines only when the read reached EOF
    let mut lines: Vec<String> = Vec::new();
    let mut total: u64 = 0;
    let mut in_script = false;
    for line in body.lines() {
        total += 1;
        if lines.len() < TEXT_LINES {
            lines.push(if is_svg {
                scrub_svg_line(line, &mut in_script)
            } else {
                scrub_line(line)
            });
        }
    }
    let truncated = lines.len() as u64 != total || !whole_file;
    Some(TextPreview {
        lines,
        truncated,
        // §B: best-effort and only when cheap — exact iff the file fit in 64 KB.
        total_lines: if whole_file { Some(total) } else { None },
        encoding: Some(
            match class {
                TextClass::Utf16Le => "utf-16le",
                TextClass::Utf16Be => "utf-16be",
                _ => "utf-8",
            }
            .to_string(),
        ),
    })
}

// ---------------------------------------------------------------------------
// HEX family — first 4 KB, 8 bytes per row (frontend regroups 8/16).
// ---------------------------------------------------------------------------

fn build_hex(path: &Path, len: u64) -> Result<HexPreview, String> {
    let raw = read_head(path, len.min(HEX_CAP))?;
    Ok(hex_from_bytes(&raw, len))
}

/// HEX rows from already-read bytes. Shared with the fallback ladder, which
/// reuses its 64 KB head when a fresh 4 KB read fails.
fn hex_from_bytes(raw: &[u8], len: u64) -> HexPreview {
    let mut rows = Vec::with_capacity(raw.len().div_ceil(8));
    for (i, chunk) in raw.chunks(8).enumerate() {
        let mut ascii = String::with_capacity(chunk.len());
        for b in chunk {
            ascii.push(if (0x20..0x7F).contains(b) { *b as char } else { '·' });
        }
        rows.push(HexRow {
            off: (i as u64) * 8,
            bytes: hex_str(chunk),
            ascii,
        });
    }
    HexPreview {
        rows,
        base: 0,
        total: len,
    }
}

fn hex_payload(raw: &[u8], len: u64) -> PreviewPayload {
    let mut payload = payload_shell("hex");
    payload.hex = Some(hex_from_bytes(raw, len));
    payload
}

// ---------------------------------------------------------------------------
// LIST family — folder listing (§G: folder tiles at deep zoom).
// ---------------------------------------------------------------------------

fn list_payload(list: ListPreview) -> PreviewPayload {
    let mut payload = payload_shell("list");
    payload.list = Some(list);
    payload
}

/// One LIST row with the §A kind tag. Directories carry no size (subtree
/// sizes need a walk — real or omitted, never fake); `method` is filled by
/// archive sources only (tar members are stored uncompressed by definition).
fn list_entry(name: String, size: Option<u64>, is_dir: bool) -> ListEntry {
    ListEntry {
        name,
        size,
        kind: if is_dir { "dir" } else { "file" }.to_string(),
        method: None,
    }
}

/// LIST footer shared by every listing source (folder walk, zip central dir,
/// sheets, tar walk, ISO PVD): truncates to LIST_CAP and flags truncation
/// when the collected rows — or a caller-supplied exact count — exceed it.
/// `kind` names the listing source for the renderer's columns.
/// WHY no shared sort: folder rows are size-sorted (§A/§G) but archives keep
/// central-dir/walk order, and reordering archive rows would change the
/// payload — so ordering deliberately stays per-source.
fn finish_list(
    mut entries: Vec<ListEntry>,
    mut truncated: bool,
    total: Option<u64>,
    kind: &str,
) -> PreviewPayload {
    if entries.len() > LIST_CAP {
        truncated = true;
    }
    entries.truncate(LIST_CAP);
    list_payload(ListPreview {
        kind: Some(kind.to_string()),
        entries,
        truncated,
        total,
    })
}

fn build_dir_list(dir: &Path, _meta: &fs::Metadata) -> Result<PreviewPayload, String> {
    let rd = fs::read_dir(dir).map_err(io_err)?;
    let mut dirs: Vec<ListEntry> = Vec::new();
    let mut files: Vec<(u64, ListEntry)> = Vec::new();
    let mut total: u64 = 0;
    let mut total_known = true;
    let mut truncated = false;
    let mut counted = 0usize;

    for entry in rd {
        let entry = entry.map_err(io_err)?;
        let file_type = entry.file_type().map_err(io_err)?;
        if file_type.is_symlink() {
            continue; // walker rule: never follow/preview symlink targets
        }
        counted += 1;
        if counted as u64 > DIR_COUNT_MAX {
            // Past the "cheap" bound a real total stops being cheap — drop it.
            truncated = true;
            total_known = false;
            break;
        }
        total += 1;
        let name = entry.file_name().to_string_lossy().into_owned();
        if dirs.len() + files.len() < DIR_HEAD {
            if file_type.is_dir() {
                dirs.push(list_entry(name, None, true));
            } else {
                let size = entry.metadata().map(|m| m.len()).ok();
                files.push((size.unwrap_or(0), list_entry(name, size, false)));
            }
        } else {
            truncated = true; // past the metadata head — sizes unsorted beyond it
        }
    }

    // Head semantics: sizes are exact for the first DIR_HEAD entries only.
    files.sort_by(|a, b| b.0.cmp(&a.0));
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    // §A/§G "size-sorted descending" applies to files; directory sizes are
    // unknown without a recursive walk, so directories lead (name-sorted).
    let mut entries: Vec<ListEntry> = dirs;
    entries.extend(files.into_iter().map(|(_, e)| e));
    Ok(finish_list(
        entries,
        truncated,
        if total_known { Some(total) } else { None },
        "folder",
    ))
}

// ---------------------------------------------------------------------------
// Family routing — §A extension table; content sniff for unknown/no-ext.
// ---------------------------------------------------------------------------

#[derive(PartialEq, Eq)]
enum Family {
    Text,
    Hex,
    Image,
    List,
    TextZip, // docx/odt → TEXT via zip/xml (§A)
    Db,
    Meta,
    Media,
    Sniff, // unknown or missing extension → magic + text/byte heuristic
}

const TEXT_EXTS: &[&str] = &[
    // Text Files + Log
    "txt", "md", "rtf", "log", "out",
    // Source Code
    "rs", "ts", "tsx", "jsx", "mjs", "cjs", "py", "java", "c", "cpp", "cc", "cxx", "h", "hpp",
    "hxx", "cs", "go", "sh", "bash", "zsh", "ps1", "bat", "cmd", "sql", "vue", "svelte", "rb",
    "php", "pl", "pm", "swift", "kt", "kts", "scala", "lua", "r", "jl", "dart", "ex", "exs",
    "erl", "hs", "ml", "asm", "s",
    // Structured Data (json/yaml/xml → Code, csv → Documents; all TEXT)
    "json", "yaml", "yml", "toml", "xml", "csv",
    // Config
    "ini", "cfg", "conf", "env", "properties", "reg", "plist",
    // 3D/CAD ascii (binary content falls through to HEX via the text guard)
    "obj", "stl", "ply", "gltf", "glb", "dxf", "step", "iges", "stp",
    // Cert/Key text + Email + Subtitle + Vector
    "pem", "crt", "cer", "asc", "key", "pub", "eml", "mbox", "srt", "ass", "ssa", "vtt",
    "svg", "eps",
];

const HEX_EXTS: &[&str] = &[
    "bin", "dat", "dmp", "pak",          // Binary
    "mdb", "accdb",                      // Access → HEX (§A)
    "tmp", "temp", "part", "crdownload", // Temporary → Caches/HEX (§A)
    "idx", "sub",                        // Subtitle index → HEX
    "fbx", "blend", "dwg",               // binary 3D/CAD → HEX
];

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif", "ico", "heic",
];

const DB_EXTS: &[&str] = &["db", "sqlite", "sqlite3"];

const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v", "ts", "m2ts"];
const AUDIO_EXTS: &[&str] = &["mp3", "wav", "flac", "m4a", "aac", "ogg", "oga", "opus", "wma"];

const LIST_EXTS: &[&str] = &["zip", "epub", "tar", "iso", "img"];
const SHEET_EXTS: &[&str] = &["xlsx", "ods"];
const ZIPXML_EXTS: &[&str] = &["docx", "odt"];

/// META family per §A (audio/video no-decode, compressed method+usize,
/// executables PE/ELF, documents, backups, firmware…).
const META_EXTS: &[&str] = &[
    // Document
    "pdf", "mobi",
    // Audio / Video (META — no decode)
    "mp3", "wav", "flac", "aac", "ogg", "oga", "opus", "m4a", "mp4", "mkv", "avi", "mov",
    "webm", "m4v",
    // Compressed (method + usize)
    "gz", "tgz", "bz2", "xz", "zst", "br", "lz4",
    // Executable (+ HEX head) + System + Firmware
    "exe", "dll", "msi", "so", "dylib", "deb", "rpm", "apk", "jar", "efi", "sys", "drv",
    "fw", "rom", "fd",
    // Vector misc / Cert binary / Email binary
    "ai", "der", "pfx", "p12", "jks", "msg", "pst", "ost",
    // Backup / Disk image / Presentation
    "bak", "backup", "bkp", "vhd", "vhdx", "wim", "cab", "pptx", "odp",
];

fn family_for_ext(ext: &str) -> Family {
    if VIDEO_EXTS.contains(&ext) || AUDIO_EXTS.contains(&ext) {
        Family::Media
    } else if TEXT_EXTS.contains(&ext) {
        Family::Text
    } else if HEX_EXTS.contains(&ext) {
        Family::Hex
    } else if IMAGE_EXTS.contains(&ext) {
        Family::Image
    } else if DB_EXTS.contains(&ext) {
        Family::Db
    } else if LIST_EXTS.contains(&ext) {
        Family::List
    } else if SHEET_EXTS.contains(&ext) {
        Family::List
    } else if ZIPXML_EXTS.contains(&ext) {
        Family::TextZip
    } else if META_EXTS.contains(&ext) {
        Family::Meta
    } else {
        Family::Sniff
    }
}

fn text_payload(t: TextPreview) -> PreviewPayload {
    let mut payload = payload_shell("text");
    payload.text = Some(t);
    payload
}

fn image_payload(i: ImagePreview) -> PreviewPayload {
    let mut payload = payload_shell("image");
    payload.image = Some(i);
    payload
}

/// MEDIA family: the playable local path PLUS cheaply parsed container facts
/// (duration/dimensions/codec) — real header bytes only, never a decode.
/// Every parse is bounded by META_HEAD; anything missing stays `None`.
fn media_payload(path: &Path, ext: &str, len: u64) -> PreviewPayload {
    let (kind, mime) = if VIDEO_EXTS.contains(&ext) {
        ("video", video_mime(ext))
    } else {
        ("audio", audio_mime(ext))
    };
    let head = read_head(path, len.min(META_HEAD)).unwrap_or_default();
    let mut facts = match ext {
        "mp4" | "m4v" | "mov" | "m4a" => {
            let mut facts = parse_mp4(&head);
            // Non-faststart muxers keep `moov` at the END — one bounded box
            // walk finds it; mvhd/tkhd sit at its head.
            if facts.duration_ms.is_none() || facts.w.is_none() {
                let moov = mp4_moov_head(path, len);
                if !moov.is_empty() {
                    mp4_header_facts(&moov, &mut facts);
                }
            }
            facts
        }
        "mkv" | "webm" => parse_mkv(&head),
        "avi" => parse_avi(&head),
        "wav" => parse_wav(&head),
        "flac" => parse_flac(&head),
        "mp3" | "aac" => parse_mp3(&head),
        "ogg" | "oga" | "opus" => parse_ogg(&head),
        _ => MediaFacts::default(),
    };
    // Signature label whenever the container parser didn't name itself.
    if !facts.pairs.iter().any(|p| p.k == "format") {
        if let Some(name) = sig_format(&head) {
            facts.pairs.insert(0, pair("format", name));
        }
    }
    let mut payload = payload_shell("media");
    payload.media = Some(MediaPreview {
        path: path.to_string_lossy().to_string(),
        mime: mime.to_string(),
        kind: kind.to_string(),
        duration_ms: facts.duration_ms,
        w: facts.w,
        h: facts.h,
        pairs: if facts.pairs.is_empty() {
            None
        } else {
            Some(facts.pairs)
        },
    });
    payload
}

fn video_mime(ext: &str) -> &'static str {
    match ext {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "ts" | "m2ts" => "video/mp2t",
        _ => "application/octet-stream",
    }
}

fn audio_mime(ext: &str) -> &'static str {
    match ext {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wma" => "audio/x-ms-wma",
        _ => "application/octet-stream",
    }
}

/// File branch: richest family first, then the §B fallback ladder (text head →
/// hex head → magic card). Every path yields `Ok` for a readable regular file;
/// `Err` survives only when the bytes genuinely cannot be read.
fn build_file(path: &Path, meta: &fs::Metadata) -> Result<PreviewPayload, String> {
    let len = meta.len();
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    // Empty file: no family can show bytes that don't exist — say so honestly.
    if len == 0 {
        let mut pairs = vec![pair("format", "empty file".to_string())];
        if let Some(d) = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        {
            pairs.push(pair("modified", fmt_unix_utc(d.as_secs())));
        }
        return Ok(meta_payload(pairs, &[], Some(0), &ext));
    }

    // `.ts` is BOTH an MPEG transport stream and TypeScript — let the bytes
    // decide before the extension table gets a say.
    let mut family = family_for_ext(&ext);
    if family == Family::Media && ext == "ts" {
        family = match read_head(path, len.min(512)) {
            Ok(head) if !is_mpegts(&head) => Family::Text,
            _ => Family::Media,
        };
    }

    match family {
        Family::Text => {
            if let Ok(Some(t)) = build_text(path, len, ext == "svg") {
                return Ok(text_payload(t));
            }
            // Binary bytes under a text extension (bplist, packed rtf) → ladder.
        }
        Family::Hex => {
            // §A maps these straight to HEX — the ladder's text rung never
            // hijacks an explicitly-binary extension.
            let head = read_head(path, len.min(HEX_CAP))?;
            return Ok(hex_payload(&head, len));
        }
        Family::Image => {
            if let Some(img) = build_image(path, len) {
                return Ok(image_payload(img));
            }
            // Unsupported codec / oversize / decode failure → magic card (§B).
            if let Ok(p) = meta_file(path, len, &ext, None) {
                return Ok(p);
            }
        }
        Family::Media => return Ok(media_payload(path, &ext, len)),
        Family::List => {
            if let Ok(p) = list_or_meta(path, len, &ext) {
                return Ok(p);
            }
        }
        Family::TextZip => {
            if let Some(t) = zip_xml_text(path, &ext) {
                return Ok(text_payload(t));
            }
            // docx/odt without the document part → zip central dir.
            if let Ok(p) = zip_list_or_meta(path, len) {
                return Ok(p);
            }
        }
        Family::Db => {
            if has_sqlite_magic(path, len) {
                if let Ok(db) = build_db(path) {
                    return Ok(db);
                }
                // Corrupt sqlite → signature META (§B) — real magic bytes.
                if let Ok(p) = meta_file(path, len, &ext, None) {
                    return Ok(p);
                }
            }
            // Non-sqlite .db (LevelDB…) → ladder rung 2 (HEX, §A).
        }
        Family::Meta => {
            if let Ok(p) = meta_file(path, len, &ext, None) {
                return Ok(p);
            }
        }
        Family::Sniff => {
            if let Ok(head) = read_head(path, len.min(META_HEAD)) {
                let attempt = match sniff_head(&head) {
                    Sniffed::Meta => meta_file(path, len, &ext, Some(head)),
                    Sniffed::Zip => zip_list_or_meta(path, len),
                    Sniffed::Tar => list_or_meta(path, len, "tar"),
                    Sniffed::Iso => list_or_meta(path, len, "iso"),
                    Sniffed::Db => {
                        build_db(path).or_else(|_| meta_file(path, len, &ext, Some(head)))
                    }
                    Sniffed::Image => match build_image(path, len) {
                        Some(img) => Ok(image_payload(img)),
                        None => meta_file(path, len, &ext, Some(head)),
                    },
                    // Text/Hex rungs from the bytes already in hand — no re-read.
                    Sniffed::Text => match text_from_head(&head, len, false) {
                        Some(t) => Ok(text_payload(t)),
                        None => Ok(hex_payload(&head, len)),
                    },
                    Sniffed::Hex => Ok(hex_payload(&head, len)),
                };
                if let Ok(p) = attempt {
                    return Ok(p);
                }
            }
        }
    }

    ladder(path, len)
}

/// Fallback ladder (§B): TEXT head (encoding-detected, from the head bytes
/// already paid for) → HEX head. The hex rung cannot fail — the 64 KB head
/// always covers the 4 KB window — so every file with one readable head gets
/// a REAL card; `Err` survives only when the head itself cannot be read.
/// (Rung 3/4 — the magic/metadata card — are `build_file`'s explicit
/// branches: Sniffed::Meta, image/db decode failures, Family::Meta.)
fn ladder(path: &Path, len: u64) -> Result<PreviewPayload, String> {
    let head = read_head(path, len.min(META_HEAD))?;

    // Rung 1: text — a text-ish file the extension table didn't map.
    if let Some(t) = text_from_head(&head, len, false) {
        return Ok(text_payload(t));
    }

    // Rung 2: hex head (§A: unknown binary stays bytes, never garbage text).
    Ok(hex_payload(&head, len))
}

/// MPEG transport streams repeat the 0x47 sync byte every 188-byte packet; a
/// lone leading 0x47 is far too weak (plain ASCII 'G' starts plenty of source
/// files), so both syncs are required.
fn is_mpegts(head: &[u8]) -> bool {
    head.len() >= 189 && head[0] == 0x47 && head[188] == 0x47
}

/// Honest type label from the FILE EXTENSION (paired with, never replacing,
/// the signature-derived `format` on the magic card).
fn ext_class(ext: &str) -> &'static str {
    match ext {
        "exe" | "dll" | "msi" | "so" | "dylib" | "efi" | "sys" | "drv" | "jar" | "apk"
        | "deb" | "rpm" => "executable",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "epub" | "mobi" | "rtf" => "document",
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "lz4" | "cab"
        | "wim" => "archive",
        "vhd" | "vhdx" | "vmdk" | "img" | "iso" | "dmg" => "disk image",
        "ttf" | "otf" | "ttc" | "woff" | "woff2" | "fon" => "font",
        "" => "unknown",
        _ if IMAGE_EXTS.contains(&ext) => "image",
        _ if VIDEO_EXTS.contains(&ext) || AUDIO_EXTS.contains(&ext) => "media",
        _ if DB_EXTS.contains(&ext) => "database",
        _ if TEXT_EXTS.contains(&ext) => "text",
        _ => "binary",
    }
}

// ---------------------------------------------------------------------------
// IMAGE family — header dims pre-check, 8 MP cap, 256 px downscale, JPEG q80.
// ---------------------------------------------------------------------------

fn build_image(path: &Path, len: u64) -> Option<ImagePreview> {
    if len == 0 || len > IMAGE_BYTES_CAP {
        return None; // oversize → meta fallback (§B "never fail the whole call")
    }
    // Header-only parse first: the 8 MP decode cap is checked BEFORE decoding.
    let (w, h) = image::image_dimensions(path).ok()?;
    if u64::from(w) * u64::from(h) > IMAGE_MP {
        return None;
    }
    let raw = read_head(path, len).ok()?;
    let img = image::load_from_memory(&raw).ok()?;
    // thumbnail() preserves aspect and clamps the LONGEST EDGE to 256 px.
    let rgb = img.thumbnail(IMAGE_EDGE, IMAGE_EDGE).to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, IMAGE_JPEG_Q)
        .encode_image(&rgb)
        .ok()?;
    use base64::Engine as _;
    let data_url = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&jpeg)
    );
    Some(ImagePreview {
        data_url,
        w, // ORIGINAL source dimensions — the plate's dimension callout.
        h,
        mime: "image/jpeg".to_string(),
    })
}

// ---------------------------------------------------------------------------
// LIST family — zip central dir / sheets / tar walk / ISO PVD.
// ---------------------------------------------------------------------------

fn zip_open(path: &Path) -> Result<zip::ZipArchive<File>, String> {
    let file = File::open(path).map_err(io_err)?;
    zip::ZipArchive::new(file).map_err(unreadable)
}

/// Reads ONE zip member's decompressed bytes with a hard cap and returns
/// `(lossy-utf8 text, hit_cap)`. Reads a single byte past the cap so callers
/// can tell "truncated at cap" from "exactly cap-sized", then drops the
/// overflow byte. Shared by the sheet manifest (xlsx/ods) and the document
/// text path (docx/odt) — both only ever need the XML head.
fn zip_part_text(
    archive: &mut zip::ZipArchive<File>,
    part: &str,
    cap: u64,
) -> Result<(String, bool), String> {
    let file = archive.by_name(part).map_err(unreadable)?;
    let mut buf = Vec::new();
    let mut limited = file.take(cap + 1);
    limited.read_to_end(&mut buf).map_err(unreadable)?;
    let hit_cap = buf.len() > cap as usize;
    buf.truncate(cap as usize);
    Ok((String::from_utf8_lossy(&buf).into_owned(), hit_cap))
}

/// LIST from a zip central directory ONLY — never extract/decompress (§B).
fn zip_list(path: &Path) -> Result<PreviewPayload, String> {
    let mut archive = zip_open(path)?;
    let total = archive.len();
    // Early-stop at the cap, not collect-then-truncate: with a corrupt
    // central dir the original code could only fail on rows it actually
    // visited, so the iteration bound is part of the observable behavior.
    let mut entries = Vec::with_capacity(total.min(LIST_CAP));
    for i in 0..total.min(LIST_CAP) {
        let file = archive.by_index(i).map_err(unreadable)?;
        let is_dir = file.is_dir();
        let mut entry = list_entry(
            file.name().to_string(),
            if is_dir { None } else { Some(file.size()) },
            is_dir,
        );
        if !is_dir {
            entry.method = Some(zip_method(file.compression()));
        }
        entries.push(entry);
    }
    Ok(finish_list(entries, total > LIST_CAP, Some(total as u64), "zip"))
}

/// Archive-tool naming for the central dir's method byte: the two common
/// methods get the conventional short names, the rest keep the crate's own
/// canonical name ("Bzip2", "Zstd", "Lzma", "Xz", "Unknown(n)"…).
fn zip_method(m: zip::CompressionMethod) -> String {
    match m {
        zip::CompressionMethod::Stored => "Store".to_string(),
        zip::CompressionMethod::Deflated => "Deflate".to_string(),
        other => other.to_string(),
    }
}

/// Runs a LIST builder; any failure falls back to signature META (§B — a bad
/// archive never fails the whole call). `ext` labels the META fallback.
fn list_or_meta_with(
    attempt: Result<PreviewPayload, String>,
    path: &Path,
    len: u64,
    ext: &str,
) -> Result<PreviewPayload, String> {
    attempt.or_else(|_| meta_file(path, len, ext, None))
}

fn zip_list_or_meta(path: &Path, len: u64) -> Result<PreviewPayload, String> {
    list_or_meta_with(zip_list(path), path, len, "zip")
}

/// xlsx/ods → LIST of SHEET names (§A): one bounded inflate of the workbook
/// manifest; falls back to the zip central dir when the part is missing.
fn sheet_list(path: &Path, ext: &str) -> Result<PreviewPayload, String> {
    let mut archive = zip_open(path)?;
    let part = if ext == "xlsx" {
        "xl/workbook.xml"
    } else {
        "content.xml"
    };
    // Bounded decompression: 64 KB of manifest is plenty for sheet names.
    let (xml, _) = zip_part_text(&mut archive, part, TEXT_CAP)?;

    // One window/attr scan serves both formats — only the open tag and the
    // attribute differ ("<sheet name=…" vs "<table:table table:name=…").
    let (open, attr) = if ext == "xlsx" {
        ("<sheet ", "name=\"") // trailing space keeps out `<sheets>`
    } else {
        ("<table:table ", "table:name=\"") // prefix keeps out -column/-row tags
    };
    let names = xml_tag_attrs(&xml, open, attr);
    if names.is_empty() {
        return Err("unsupported".to_string()); // caller falls back to central dir
    }
    let total = names.len();
    let entries: Vec<ListEntry> = names
        .into_iter()
        .map(|name| list_entry(name, None, false))
        .collect();
    Ok(finish_list(entries, total > LIST_CAP, Some(total as u64), "sheet"))
}

/// Collects `attr` values from every `open`-tag occurrence, scanning a
/// 240-char window after each tag start (long enough for the attribute,
/// short enough to stay within the manifest head).
fn xml_tag_attrs(xml: &str, open: &str, attr: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = xml;
    while let Some(pos) = rest.find(open) {
        let window = &rest[pos..pos.saturating_add(240).min(rest.len())];
        if let Some(nq) = window.find(attr) {
            let start = nq + attr.len();
            if let Some(end) = window[start..].find('"') {
                let name = unescape_xml(&window[start..start + end]);
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
        rest = &rest[pos + open.len()..];
    }
    names
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#10;", "\n")
}

/// tar → sequential 512-byte header walk (uncompressed; no crate needed and
/// none allowed — flate2 isn't on the §B list, so tar.gz stays META).
fn tar_list(path: &Path) -> Result<PreviewPayload, String> {
    const WALK_MAX: usize = LIST_CAP + 1; // walk one past the cap to detect truncation
    let mut entries: Vec<ListEntry> = Vec::new();
    let mut offset = 0u64;
    let mut pending_name: Option<String> = None; // GNU 'L' / pax 'x' override
    let mut truncated = false;

    while entries.len() < WALK_MAX {
        let block = read_at(path, offset, 512)?;
        if block.len() < 512 {
            break; // EOF mid-header
        }
        if block[0] == 0 {
            break; // zero block = end of archive → total below is exact
        }
        if offset == 0 && !tar_checksum_ok(&block) {
            return Err("unsupported".to_string()); // not actually a tar
        }
        let size = tar_size(&block)?;
        let typeflag = block[156];
        let name = pending_name.take().unwrap_or_else(|| tar_name(&block));
        let data_off = offset + 512;

        match typeflag {
            // Long-name members: their CONTENT is the next member's name.
            b'L' | b'x' => {
                let cap = size.min(1024);
                let raw = read_at(path, data_off, cap)?;
                let text = String::from_utf8_lossy(&raw);
                let mut name = text.trim_end_matches('\0').trim().to_string();
                if typeflag == b'x' {
                    // pax records are "NN key=value\n" — prefer the path= value.
                    if let Some(p) = text.find("path=") {
                        let value = text[p + 5..]
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        if !value.is_empty() {
                            name = value;
                        }
                    }
                }
                if !name.is_empty() {
                    pending_name = Some(name);
                }
            }
            _ => {
                if !name.is_empty() {
                    let mut entry = list_entry(
                        name.trim_end_matches("/").to_string(),
                        Some(size),
                        typeflag == b'5',
                    );
                    if typeflag != b'5' {
                        // tar members are uncompressed by definition.
                        entry.method = Some("Store".to_string());
                    }
                    entries.push(entry);
                }
            }
        }
        offset = data_off + size.div_ceil(512) * 512;
        if offset > 1 << 40 {
            break; // sanity: absurd header sizes are not followed
        }
    }

    if entries.is_empty() {
        // Zero first block (empty tar or binary junk) — no listing to trust.
        return Err("unsupported".to_string());
    }
    let total = entries.len();
    truncated |= total > LIST_CAP;
    Ok(finish_list(
        entries,
        truncated,
        if truncated { None } else { Some(total as u64) },
        "tar",
    ))
}

/// POSIX/GNU header validity: header checksum field must match the bytes.
fn tar_checksum_ok(block: &[u8]) -> bool {
    let stored = match tar_octal(&block[148..156]) {
        Some(v) => v,
        None => return false,
    };
    let mut unsigned = 0u64;
    let mut signed = 0i32;
    for (i, &b) in block.iter().enumerate() {
        let byte = if (148..156).contains(&i) { b' ' } else { b };
        unsigned += u64::from(byte);
        signed += i32::from(byte as i8);
    }
    stored == unsigned || stored as i64 == i64::from(signed)
}

fn tar_octal(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|&b| b & 0x80 != 0) {
        // GNU base-256 size encoding (huge members).
        let mut acc: u64 = u64::from(field[0] & 0x7F);
        for &b in &field[1..] {
            acc = acc.checked_mul(256)?.checked_add(u64::from(b))?;
        }
        return Some(acc);
    }
    let text: Vec<u8> = field
        .iter()
        .copied()
        .take_while(|b| *b != 0 && *b != b' ')
        .collect();
    if text.is_empty() {
        return Some(0);
    }
    std::str::from_utf8(&text).ok()?.parse().ok()
}

fn tar_size(block: &[u8]) -> Result<u64, String> {
    tar_octal(&block[124..136]).ok_or_else(|| "unreadable: bad tar header".to_string())
}

fn tar_name(block: &[u8]) -> String {
    let base = cstr_field(&block[0..100]);
    // ustar prefix (345..500) extends the name.
    let prefix = cstr_field(&block[345..500]);
    if prefix.is_empty() {
        base
    } else {
        format!("{prefix}/{base}")
    }
}

fn cstr_field(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).trim().to_string()
}

/// ISO9660: PVD at sector 16 → root directory record → real dir entries.
fn iso_list(path: &Path) -> Result<PreviewPayload, String> {
    let pvd = read_at(path, 16 * 2048, 2048)?;
    if pvd.len() < 2048 || pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return Err("unsupported".to_string()); // not ISO (raw .img etc.)
    }
    let block_size = u16::from_le_bytes([pvd[128], pvd[129]]);
    let block_size = if block_size == 0 { 2048 } else { block_size };
    let root = &pvd[156..190];
    let extent = u32::from_le_bytes([root[2], root[3], root[4], root[5]]);
    let dir_len = u32::from_le_bytes([root[10], root[11], root[12], root[13]]);
    if dir_len == 0 {
        return Err("unsupported".to_string());
    }
    let read_n = u64::from(dir_len).min(64 * 1024); // bounded directory read
    let data = read_at(path, u64::from(extent) * u64::from(block_size), read_n)?;
    let full_dir = u64::from(dir_len) == read_n;

    let mut entries: Vec<ListEntry> = Vec::new();
    let mut total = 0u64;
    let mut pos = 0usize;
    while pos < data.len() {
        let rec_len = data[pos] as usize;
        if rec_len == 0 {
            break; // padding to the next block
        }
        if pos + rec_len > data.len() || rec_len < 34 {
            break;
        }
        let rec = &data[pos..pos + rec_len];
        let name_len = rec[32] as usize;
        if 33 + name_len <= rec_len {
            let raw_name = &rec[33..33 + name_len];
            // Skip the "\x00" parent and "\x01" current-directory records.
            if !raw_name.is_empty() && raw_name[0] != 0 && raw_name[0] != 1 {
                let mut name = String::from_utf8_lossy(raw_name).into_owned();
                // ISO version suffix ";1" is noise in a preview.
                if let Some(p) = name.find(';') {
                    name.truncate(p);
                }
                let is_dir = rec[25] & 0x02 != 0;
                let size = u32::from_le_bytes([rec[10], rec[11], rec[12], rec[13]]);
                total += 1;
                if entries.len() < LIST_CAP {
                    entries.push(list_entry(
                        name,
                        if is_dir { None } else { Some(u64::from(size)) },
                        is_dir,
                    ));
                }
            }
        }
        pos += rec_len;
    }
    if entries.is_empty() {
        return Err("unsupported".to_string());
    }
    // `total` counts every record (push is capped), so truncation is driven
    // by the exact total — not by the collected rows.
    Ok(finish_list(
        entries,
        !full_dir || total > LIST_CAP as u64,
        if full_dir { Some(total) } else { None },
        "iso",
    ))
}

/// Dispatch for LIST-mapped extensions with a META fallback (§B).
fn list_or_meta(path: &Path, len: u64, ext: &str) -> Result<PreviewPayload, String> {
    let attempt = match ext {
        "zip" | "epub" => zip_list(path),
        "xlsx" | "ods" => sheet_list(path, ext).or_else(|_| zip_list(path)),
        "tar" => tar_list(path),
        "iso" | "img" => iso_list(path),
        _ => zip_list(path),
    };
    list_or_meta_with(attempt, path, len, ext)
}

// ---------------------------------------------------------------------------
// TEXT via zip/xml — docx/odt (§A "TEXT (zip/xml)").
// ---------------------------------------------------------------------------

/// Extracts the document part with a 64 KB decompression cap, strips tags to
/// plain lines; `None` on any failure — the caller falls back to a zip LIST.
fn zip_xml_text(path: &Path, ext: &str) -> Option<TextPreview> {
    let mut archive = zip_open(path).ok()?;
    let part = if ext == "docx" {
        "word/document.xml"
    } else {
        "content.xml"
    };
    // Missing part / read error → None: the caller falls back to a LIST.
    let (xml, hit_cap) = zip_part_text(&mut archive, part, TEXT_CAP).ok()?;

    // XML → text via an explicit tag walk: paragraph/cell closes and line
    // breaks become newlines, tabs stay tabs, run/span markup is dropped so
    // text pieces rejoin instead of fracturing per run.
    let bytes = xml.as_bytes();
    let mut i = 0usize;
    let mut text = String::with_capacity(xml.len());
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let end = xml[i..].find('>').map(|p| i + p);
            let tag = match end {
                Some(e) => &xml[i..e],
                None => break,
            };
            let is_close = tag.starts_with("</");
            let inner = tag
                .trim_start_matches("</")
                .trim_start_matches('<')
                .trim_end_matches('/')
                .trim();
            if is_close
                && (inner.ends_with(":p")
                    || inner.ends_with(":tr")
                    || inner.ends_with(":tc")
                    || inner == "p"
                    || inner == "tr"
                    || inner == "li")
            {
                text.push('\n');
            } else if inner.starts_with("w:tab") || inner.starts_with("text:tab") {
                text.push('\t');
            } else if inner.starts_with("w:br")
                || inner.starts_with("text:line-break")
                || inner == "br"
            {
                text.push('\n');
            } else if inner.starts_with("text:s") && !inner.starts_with("text:se") {
                text.push(' ');
            }
            i = end.unwrap_or(bytes.len());
        } else {
            let ch = xml[i..].chars().next();
            match ch {
                Some(c) => {
                    text.push(c);
                    i += c.len_utf8();
                }
                None => break,
            }
        }
    }
    let text = unescape_xml(&text);

    let mut lines = Vec::new();
    let mut total = 0u64;
    for line in text.lines() {
        let line = scrub_line(line);
        if !line.trim().is_empty() {
            total += 1;
            if lines.len() < TEXT_LINES {
                lines.push(line);
            }
        }
    }
    if lines.is_empty() && total == 0 {
        return None; // nothing readable → caller falls back to LIST
    }
    Some(TextPreview {
        truncated: hit_cap || total > TEXT_LINES as u64,
        total_lines: if hit_cap { None } else { Some(total) },
        lines,
        encoding: Some("utf-8".to_string()),
    })
}

// ---------------------------------------------------------------------------
// DB family — read-only URI, tables + column counts only (no COUNT(*)).
// ---------------------------------------------------------------------------

fn sqlite_ro_uri(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(raw.len() + 8);
    for c in raw.chars() {
        match c {
            '%' => encoded.push_str("%25"),
            '#' => encoded.push_str("%23"),
            '?' => encoded.push_str("%3F"),
            '"' | '\'' | ' ' => {
                encoded.push('%');
                encoded.push_str(&format!("{:02X}", c as u32));
            }
            _ => encoded.push(c),
        }
    }
    format!("file:{encoded}?mode=ro")
}

fn has_sqlite_magic(path: &Path, len: u64) -> bool {
    read_head(path, len.min(16))
        .map(|b| b.starts_with(b"SQLite format 3\x00"))
        .unwrap_or(false)
}

fn build_db(path: &Path) -> Result<PreviewPayload, String> {
    let conn = rusqlite::Connection::open_with_flags(
        sqlite_ro_uri(path),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(unreadable)?;
    // Bounded lock wait — a busy database degrades to the META fallback.
    let _ = conn.busy_timeout(std::time::Duration::from_millis(250));

    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .map_err(unreadable)?;
    let names: Vec<String> = {
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(unreadable)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(unreadable)?);
        }
        out
    };
    drop(stmt);

    let truncated = names.len() > DB_TABLES_CAP;
    // Column NAMES double as the count — one pragma pass per table; a table
    // whose pragma fails (virtual/corrupt) degrades to `columns: null`, it
    // never sinks the whole payload.
    let mut col_stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?)")
        .map_err(unreadable)?;
    let mut tables = Vec::with_capacity(names.len().min(DB_TABLES_CAP));
    for (i, name) in names.iter().take(DB_TABLES_CAP).enumerate() {
        let columns: Option<Vec<String>> = col_stmt
            .query_map([name.as_str()], |row| row.get::<_, String>(0))
            .ok()
            .map(|rows| rows.filter_map(Result::ok).collect());
        // First rows of the leading tables: bounded LIMIT, display-capped
        // cells; per-table errors leave `rows` absent (schema still real).
        let rows = match &columns {
            Some(cols) if !cols.is_empty() && i < DB_ROWS_TABLES => db_sample_rows(&conn, name),
            _ => None,
        };
        tables.push(DbTable {
            name: name.clone(),
            cols: columns.as_ref().map(Vec::len).unwrap_or(0).min(u32::MAX as usize) as u32,
            columns,
            rows,
        });
    }

    let mut payload = payload_shell("db");
    payload.db = Some(DbPreview { tables, truncated });
    Ok(payload)
}

/// First rows of one table, cell-by-cell display text. `None` on any error
/// (locked, corrupt btree, insane schema) — the schema card stands alone.
fn db_sample_rows(conn: &rusqlite::Connection, table: &str) -> Option<Vec<Vec<String>>> {
    let quoted = table.replace('"', "\"\"");
    let sql = format!("SELECT * FROM \"{quoted}\" LIMIT {DB_ROWS}");
    let mut stmt = conn.prepare(&sql).ok()?;
    // Wide tables show a bounded head of their columns, never the full width.
    let ncols = stmt.column_count().min(24);
    let mut rows = stmt.query([]).ok()?;
    let mut out: Vec<Vec<String>> = Vec::with_capacity(DB_ROWS);
    while let Some(row) = rows.next().ok()? {
        let mut cells = Vec::with_capacity(ncols);
        for i in 0..ncols {
            cells.push(db_cell(row.get_ref(i).ok()));
        }
        out.push(cells);
    }
    Some(out)
}

/// One sampled cell as honest display text: NULL/integer/real/scrubbed text;
/// blobs are a byte-size stub, never raw bytes.
fn db_cell(v: Option<rusqlite::types::ValueRef<'_>>) -> String {
    use rusqlite::types::ValueRef;
    let text = match v {
        None | Some(ValueRef::Null) => "null".to_string(),
        Some(ValueRef::Integer(i)) => i.to_string(),
        Some(ValueRef::Real(f)) => format!("{f}"),
        Some(ValueRef::Text(t)) => scrub_line(&String::from_utf8_lossy(t)),
        Some(ValueRef::Blob(b)) => return format!("<blob {} B>", b.len()),
    };
    if text.chars().count() > DB_CELL {
        let cut: String = text.chars().take(DB_CELL).collect();
        format!("{cut}...")
    } else {
        text
    }
}

// ---------------------------------------------------------------------------
// Content sniff for unknown/no-extension files (§A lists no-ext as Binary;
// content truth wins here — magic routes to the real family, text heuristics
// decide TEXT vs HEX).
// ---------------------------------------------------------------------------

enum Sniffed {
    Meta,
    Zip,
    Tar,
    Iso,
    Db,
    Image,
    Text,
    Hex,
}

fn sniff_head(head: &[u8]) -> Sniffed {
    let at = |off: usize, sig: &[u8]| head.len() >= off + sig.len() && &head[off..off + sig.len()] == sig;

    if head.starts_with(b"SQLite format 3\x00") {
        return Sniffed::Db;
    }
    if head.starts_with(b"MZ") || head.starts_with(b"\x7fELF") || head.starts_with(b"%PDF") {
        return Sniffed::Meta;
    }
    if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Sniffed::Meta; // OLE compound: msi/doc/pst…
    }
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") || head.starts_with(b"PK\x07\x08")
    {
        return Sniffed::Zip;
    }
    if head.starts_with(&[0x1F, 0x8B])
        || head.starts_with(b"BZh")
        || head.starts_with(b"\xfd7zXZ\x00")
        || head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD])
        || head.starts_with(&[0x04, 0x22, 0x4D, 0x18])
        || head.starts_with(b"Rar!")
        || head.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C])
        || head.starts_with(b"MSCF")
        || head.starts_with(b"!<arch>\n")
        || head.starts_with(&[0x8E, 0xAD, 0xE8])
        || head.starts_with(b"BOOKMOBI")
        || at(60, b"BOOKMOBI")
        || at(60, b"TEXtREAd")
        || head.starts_with(b"MSWIM\x00\x00\x00")
        || head.starts_with(&[0x63, 0x6F, 0x6E, 0x65, 0x63, 0x74, 0x69, 0x78]) // vhd
        || head.starts_with(b"vhdx")
        || head.starts_with(b"bplist00")
        || head.starts_with(b"\x00asm")
        || head.starts_with(b"fLaC")
        || head.starts_with(b"OggS")
        || head.starts_with(b"!BDN")
        || (head.starts_with(b"RIFF") && !at(8, b"WEBP"))
        || at(4, b"ftyp")
        || head.starts_with(b"wOFF")
        || head.starts_with(b"wOF2")
        || head.starts_with(b"ttcf")
        || head.starts_with(b"OTTO")
        || head.starts_with(&[0x00, 0x01, 0x00, 0x00])
        || head.starts_with(b"ID3")
        || (head.len() >= 4 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0)
    {
        return Sniffed::Meta;
    }
    if at(257, b"ustar") {
        return Sniffed::Tar;
    }
    if at(0x8001, b"CD001") {
        return Sniffed::Iso;
    }
    // Raster magics → IMAGE (decode path, falls back to META on failure).
    if head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
        || head.starts_with(b"GIF87a")
        || head.starts_with(b"GIF89a")
        || head.starts_with(&[0x49, 0x49, 0x2A, 0x00])
        || head.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
        || head.starts_with(&[0x00, 0x00, 0x01, 0x00])
        || at(8, b"WEBP")
        || (head.starts_with(b"BM")
            && head.len() >= 18
            && matches!(
                u32::from_le_bytes([head[14], head[15], head[16], head[17]]),
                12 | 40 | 52 | 56 | 64 | 108 | 124
            ))
    {
        return Sniffed::Image;
    }
    match classify_text(head) {
        TextClass::Binary => Sniffed::Hex,
        _ => Sniffed::Text,
    }
}

// ---------------------------------------------------------------------------
// META family — real parsed bytes only.
// ---------------------------------------------------------------------------

/// Recognized container/format signature → display name (real magic bytes).
fn sig_format(head: &[u8]) -> Option<String> {
    let at = |off: usize, sig: &[u8]| head.len() >= off + sig.len() && &head[off..off + sig.len()] == sig;
    let s: Option<String> = if head.starts_with(b"PK\x03\x04")
        || head.starts_with(b"PK\x05\x06")
        || head.starts_with(b"PK\x07\x08")
    {
        Some("zip".to_string())
    } else if head.starts_with(&[0x1F, 0x8B]) {
        Some("gzip".to_string())
    } else if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        // EBML: DocType ("webm"/"matroska") is the header's first child.
        let window = &head[..head.len().min(1024)];
        if window.windows(4).any(|w| w == b"webm") {
            Some("webm")
        } else if window.windows(8).any(|w| w == b"matroska") {
            Some("matroska")
        } else {
            Some("ebml")
        }
        .map(str::to_string)
    } else if head.starts_with(b"BZh") {
        Some(format!(
            "bzip2 level {}",
            // `get`, not indexing: a 3-byte truncated head must not panic.
            head.get(3).map_or(0, |b| b.saturating_sub(b'0')).min(9)
        ))
    } else if head.starts_with(b"\xfd7zXZ\x00") {
        Some("xz".to_string())
    } else if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        Some("zstd".to_string())
    } else if head.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        Some("lz4".to_string())
    } else if head.starts_with(b"Rar!") {
        Some(
            if at(6, &[0x01]) {
                "rar v5".to_string()
            } else {
                "rar v4".to_string()
            },
        )
    } else if head.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        let v = if head.len() >= 8 {
            format!(".{}", head[7]) // major/minor version bytes (BC AF … v?)
        } else {
            String::new()
        };
        Some(format!("7z{v}"))
    } else if head.starts_with(b"MSCF") {
        Some("cab".to_string())
    } else if head.starts_with(b"!<arch>\n") {
        Some("ar".to_string())
    } else if head.starts_with(&[0x8E, 0xAD, 0xE8]) {
        Some("rpm".to_string())
    } else if head.starts_with(b"SQLite format 3\x00") {
        Some("sqlite3".to_string())
    } else if head.starts_with(b"%PDF-") && head.len() >= 8 {
        let ver = String::from_utf8_lossy(&head[5..8]);
        if ver.chars().all(|c| c.is_ascii_digit() || c == '.') {
            Some(format!("pdf {ver}"))
        } else {
            Some("pdf".to_string())
        }
    } else if head.starts_with(b"fLaC") {
        Some("flac".to_string())
    } else if head.starts_with(b"OggS") {
        Some("ogg".to_string())
    } else if head.starts_with(b"RIFF") && head.len() >= 12 {
        Some(
            match &head[8..12] {
                b"WAVE" => "wav",
                b"AVI " => "avi",
                b"WEBP" => "webp",
                _ => "riff",
            }
            .to_string(),
        )
    } else if at(4, b"ftyp") && head.len() >= 12 {
        let brand = String::from_utf8_lossy(&head[8..12]).trim().to_string();
        Some(format!("mp4 ftyp {brand}"))
    } else if head.starts_with(b"MSWIM\x00\x00\x00") {
        Some("wim".to_string())
    } else if head.starts_with(&[0x63, 0x6F, 0x6E, 0x65, 0x63, 0x74, 0x69, 0x78]) {
        Some("vhd".to_string())
    } else if head.starts_with(b"vhdx") {
        Some("vhdx".to_string())
    } else if head.starts_with(b"bplist00") && head.len() >= 9 {
        Some(format!(
            "binary plist {}",
            String::from_utf8_lossy(&head[8..9])
        ))
    } else if head.starts_with(&[0x00, 0x61, 0x73, 0x6D]) {
        Some("wasm".to_string())
    } else if at(0x8001, b"CD001") {
        Some("iso9660".to_string())
    } else if at(257, b"ustar") {
        Some("tar".to_string())
    } else if head.starts_with(b"!BDN") {
        Some("outlook pst".to_string())
    } else if at(60, b"BOOKMOBI") {
        Some("mobi".to_string())
    } else if head.starts_with(b"wOFF") {
        Some("woff".to_string())
    } else if head.starts_with(b"wOF2") {
        Some("woff2".to_string())
    } else if head.starts_with(b"ttcf") {
        Some("ttc".to_string())
    } else if head.starts_with(b"OTTO") {
        Some("opentype cff".to_string())
    } else if head.starts_with(&[0x00, 0x01, 0x00, 0x00]) {
        Some("truetype".to_string())
    } else if head.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png".to_string())
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg".to_string())
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("gif".to_string())
    } else if head.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || head.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        Some("tiff".to_string())
    } else if head.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        Some("ico".to_string())
    } else if head.starts_with(b"BM") {
        Some("bmp".to_string())
    } else if head.starts_with(b"MZ") {
        Some("pe (mz)".to_string())
    } else if head.starts_with(b"\x7fELF") {
        Some("elf".to_string())
    } else if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        Some("ole compound".to_string())
    } else if head.starts_with(b"BOOKMOBI") {
        Some("mobi".to_string())
    } else if head.starts_with(b"ID3") {
        Some("mp3".to_string())
    } else {
        // Extension-provenance as a LAST resort would be un-real data — when
        // nothing in the bytes is recognized we report the raw signature
        // instead (see meta_payload).
        None
    };
    s
}

/// META builder: bounded head read, content parsers, then the §B fallback
/// (signature hex + size). `ext` only selects the few archive counters
/// (pptx/jar) that need a central-directory open.
fn meta_file(path: &Path, len: u64, ext: &str, head: Option<Vec<u8>>) -> Result<PreviewPayload, String> {
    let head = match head {
        Some(h) => h,
        None => read_head(path, len.min(META_HEAD))?,
    };

    let mut pairs: Vec<MetaPair> = Vec::new();
    let mut pe_or_elf = false;

    // --- Executables: PE/ELF header — arch, sections, image size (§A). ---
    if head.starts_with(b"MZ") {
        if let Some(p) = parse_pe(path, &head) {
            pairs = p;
            pe_or_elf = true;
        }
    } else if head.starts_with(b"\x7fELF") {
        if let Some(p) = parse_elf(path, &head) {
            pairs = p;
            pe_or_elf = true;
        }
    } else if head.starts_with(b"%PDF") {
        pairs = parse_pdf(path, len).unwrap_or_default();
    } else if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        pairs = parse_cfb(path, &head).unwrap_or_default();
    } else if is_font_sig(&head) {
        pairs = parse_font(path, &head).unwrap_or_default();
    }

    // --- Cheap container details (all bounded header reads). ---
    if pairs.is_empty() {
        if head.starts_with(&[0x1F, 0x8B]) {
            pairs = parse_gzip(path, len, &head);
        } else if head.starts_with(b"RIFF") && head.len() >= 12 && &head[8..12] == b"WAVE" {
            pairs = parse_wav(&head).pairs;
        } else if head.starts_with(b"fLaC") {
            pairs = parse_flac(&head).pairs;
        } else if head.starts_with(b"OggS") {
            pairs = parse_ogg(&head).pairs;
        } else if head.starts_with(b"ID3")
            || (head.len() >= 4 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0)
        {
            pairs = parse_mp3(&head).pairs;
        } else if head.len() >= 12 && &head[4..8] == b"ftyp" {
            pairs = parse_mp4(&head).pairs;
        } else if at60_mobi(&head) {
            pairs = parse_mobi(&head);
        } else if head.starts_with(b"!<arch>\n") {
            pairs = parse_ar(&head);
        } else if head.starts_with(b"MSCF") {
            pairs = parse_cab(&head);
        }
    }

    // --- Archive counters keyed by extension (central dir / manifest). ---
    if pairs.is_empty() {
        match ext {
            "pptx" | "odp" => pairs = parse_slides(path, ext),
            "jar" | "apk" => pairs = parse_zip_count(path),
            _ => {}
        }
    }

    // --- Generic signature label when nothing richer parsed. ---
    if !pairs.iter().any(|p| p.k == "format") {
        if let Some(name) = sig_format(&head) {
            pairs.push(pair("format", name));
        }
    }

    // Real mtime from the canonicalized path — one stat, never a guess.
    if let Ok(t) = fs::metadata(path).and_then(|m| m.modified()) {
        if let Ok(d) = t.duration_since(UNIX_EPOCH) {
            pairs.push(pair("modified", fmt_unix_utc(d.as_secs())));
        }
    }

    let mut payload = meta_payload(pairs, &head, Some(len), ext);

    // §A: executables/system/firmware also carry the HEX head. Best-effort —
    // a failed hex read never sinks the metadata card that already parsed.
    const HEX_HEAD_EXTS: &[&str] = &[
        "exe", "dll", "msi", "so", "dylib", "sys", "drv", "efi", "fw", "rom", "fd", "deb", "rpm",
    ];
    if pe_or_elf || HEX_HEAD_EXTS.contains(&ext) {
        payload.hex = build_hex(path, len).ok();
    }
    Ok(payload)
}

fn at60_mobi(head: &[u8]) -> bool {
    (head.len() >= 68 && &head[60..68] == b"BOOKMOBI") || head.starts_with(b"BOOKMOBI")
}

fn is_font_sig(head: &[u8]) -> bool {
    head.starts_with(b"wOFF")
        || head.starts_with(b"wOF2")
        || head.starts_with(b"ttcf")
        || head.starts_with(b"OTTO")
        || head.starts_with(&[0x00, 0x01, 0x00, 0x00])
        || head.starts_with(b"true")
}

// ---------------------------------------------------------------------------
// Byte readers — every parser below reads REAL bytes only (R5/INS_BRIEF).
// ---------------------------------------------------------------------------

fn u16le(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off + 2)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn u32le(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64le(b: &[u8], off: usize) -> Option<u64> {
    let s = b.get(off..off + 8)?;
    Some(u64::from_le_bytes(s.try_into().ok()?))
}

fn u16be(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off + 2)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

fn u32be(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn ascii4(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim().to_string()
}

// --- PE (Windows executables/dlls/sys/drv/msi headers) ---

fn machine_name(machine: u16) -> &'static str {
    match machine {
        0x8664 => "x64",
        0x014C => "x86",
        0xAA64 => "arm64",
        0x01C0 | 0x01C4 => "arm",
        0x0200 => "ia64",
        0x5032 => "riscv32",
        0x5064 => "riscv64",
        _ => "unknown",
    }
}

fn subsystem_name(subsystem: u16) -> &'static str {
    match subsystem {
        1 => "native",
        2 => "windows gui",
        3 => "windows console",
        5 => "os/2 console",
        7 => "posix console",
        9 => "windows ce gui",
        10 => "efi application",
        11 => "efi service",
        12 => "efi runtime",
        13 => "efi rom",
        14 => "xbox",
        16 => "windows boot",
        _ => "unknown",
    }
}

fn parse_pe(path: &Path, head: &[u8]) -> Option<Vec<MetaPair>> {
    if head.len() < 0x40 {
        return None;
    }
    let e_lfanew = u64::from(u32le(head, 0x3C)?);
    let hdr: Vec<u8> = if e_lfanew.checked_add(64)? <= head.len() as u64 {
        head[e_lfanew as usize..].to_vec()
    } else {
        // Large DOS stub — one bounded targeted read.
        read_at(path, e_lfanew, TABLE_READ_CAP).ok()?
    };
    if hdr.len() < 24 || &hdr[0..4] != b"PE\0\0" {
        return None;
    }
    let machine = u16le(&hdr, 4)?;
    let n_sections = u16le(&hdr, 6)?;
    let opt_size = usize::from(u16le(&hdr, 8)?);
    let chars = u16le(&hdr, 10)?;
    let timestamp = u32le(&hdr, 12)?;

    let opt = hdr.get(20..)?;
    let opt_magic = u16le(opt, 0)?;
    let format: Option<&str> = match opt_magic {
        0x10B => Some("PE32"),
        0x20B => Some("PE32+"),
        0x107 => Some("PE ROM"),
        _ => None,
    };

    let mut pairs: Vec<MetaPair> = Vec::new();
    if let Some(f) = format {
        pairs.push(pair("format", f.to_string()));
    }
    pairs.push(pair("arch", machine_name(machine).to_string()));
    pairs.push(pair(
        "kind",
        if chars & 0x2000 != 0 { "dll" } else { "executable" }.to_string(),
    ));
    // Optional-header fields share offsets in PE32 and PE32+.
    if format.is_some() && opt.len() >= 70 {
        let image_size = u32le(opt, 56)?;
        let subsystem = u16le(opt, 68)?;
        pairs.push(pair("image size", fmt_bytes(u64::from(image_size))));
        pairs.push(pair("subsystem", subsystem_name(subsystem).to_string()));
    }
    // Section table: 40-byte records with the 8-byte ASCII names.
    let mut names: Vec<String> = Vec::new();
    for i in 0..usize::from(n_sections) {
        let start = 20 + opt_size + i * 40;
        let raw = match hdr.get(start..start + 8) {
            Some(r) => r,
            None => break,
        };
        let end = raw.iter().position(|&b| b == 0).unwrap_or(8);
        let name = String::from_utf8_lossy(&raw[..end]).trim().to_string();
        if !name.is_empty() {
            names.push(name);
        }
    }
    if n_sections > 0 {
        pairs.push(pair("sections", n_sections.to_string()));
        if !names.is_empty() {
            names.truncate(10);
            pairs.push(pair("section names", names.join(" ")));
        }
    }
    if timestamp != 0 {
        // PE TimeDateStamp is a real Unix timestamp (PE spec).
        pairs.push(pair("build time", fmt_unix_utc(u64::from(timestamp))));
    }
    Some(pairs)
}

// --- ELF (Linux/macOS objects, .so) ---

fn elf_machine(machine: u16) -> &'static str {
    match machine {
        0x03 => "x86",
        0x3E => "x86-64",
        0x28 => "arm",
        0xB7 => "aarch64",
        0xF3 => "riscv",
        0x14 => "ppc",
        0x15 => "ppc64",
        0x2A => "superh",
        _ => "unknown",
    }
}

fn elf_type(e_type: u16) -> &'static str {
    match e_type {
        1 => "relocatable",
        2 => "executable",
        3 => "shared object",
        4 => "core",
        _ => "none",
    }
}

fn parse_elf(path: &Path, head: &[u8]) -> Option<Vec<MetaPair>> {
    if head.len() < 52 {
        return None;
    }
    let is64 = head[4] == 2;
    let le = head[5] != 2;
    if is64 && head.len() < 64 {
        return None;
    }
    let rd16 = |off: usize| -> Option<u16> {
        if le { u16le(head, off) } else { u16be(head, off) }
    };
    let rd32 = |off: usize| -> Option<u32> {
        if le { u32le(head, off) } else { u32be(head, off) }
    };
    let rd64 = |off: usize| -> Option<u64> {
        if le { u64le(head, off) } else {
            let s = head.get(off..off + 8)?;
            Some(u64::from_be_bytes(s.try_into().ok()?))
        }
    };

    let e_type = rd16(16)?;
    let e_machine = rd16(18)?;
    let (entry, shoff, shentsize, shnum, shstrndx) = if is64 {
        (rd64(24)?, rd64(40)?, rd16(58)?, rd16(60)?, rd16(62)?)
    } else {
        (
            u64::from(rd32(24)?),
            u64::from(rd32(32)?),
            rd16(46)?,
            rd16(48)?,
            rd16(50)?,
        )
    };

    let mut pairs = vec![
        pair(
            "format",
            format!(
                "ELF{} {}",
                if is64 { "64" } else { "32" },
                if le { "LSB" } else { "MSB" }
            ),
        ),
        pair("arch", elf_machine(e_machine).to_string()),
        pair("kind", elf_type(e_type).to_string()),
    ];
    if shnum > 0 {
        pairs.push(pair("sections", shnum.to_string()));
        // Section NAMES via the section-header string table (bounded reads).
        if shnum <= 96 && shoff > 0 && (shentsize == 40 || shentsize == 64) && shstrndx < shnum {
            let names = elf_section_names(path, shoff, shentsize, shnum, shstrndx, is64);
            if !names.is_empty() {
                pairs.push(pair("section names", names.join(" ")));
            }
        }
    }
    if entry != 0 {
        pairs.push(pair("entry", format!("0x{entry:x}")));
    }
    Some(pairs)
}

fn elf_section_names(
    path: &Path,
    shoff: u64,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
    is64: bool,
) -> Vec<String> {
    let total = u64::from(shnum) * u64::from(shentsize);
    if total == 0 || total > 16 * 1024 {
        return Vec::new(); // bounded: shnum ≤ 96 → ≤ 6 KB
    }
    let table = match read_at(path, shoff, total) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let shstr = match table.get(shstrndx as usize * usize::from(shentsize)..)
        .and_then(|s| s.get(..usize::from(shentsize)))
    {
        Some(s) => s,
        None => return Vec::new(),
    };
    let (str_off, str_size) = if is64 {
        match (u64le(shstr, 24), u64le(shstr, 32)) {
            (Some(o), Some(s)) => (o, s),
            _ => return Vec::new(),
        }
    } else {
        match (u32le(shstr, 16), u32le(shstr, 20)) {
            (Some(o), Some(s)) => (u64::from(o), u64::from(s)),
            _ => return Vec::new(),
        }
    };
    if str_size == 0 || str_size > 64 * 1024 {
        return Vec::new();
    }
    let strtab = match read_at(path, str_off, str_size) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut names = Vec::new();
    for i in 0..usize::from(shnum) {
        let header = match table.get(i * usize::from(shentsize)..i * usize::from(shentsize) + 4) {
            Some(h) => h,
            None => break,
        };
        let name_off = match u32le(header, 0) {
            Some(o) => o as usize,
            None => break,
        };
        let name = match strtab.get(name_off..).map(|s| {
            let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
            String::from_utf8_lossy(&s[..end]).trim().to_string()
        }) {
            Some(n) if !n.is_empty() => n,
            _ => continue,
        };
        names.push(name);
        if names.len() >= 12 {
            break;
        }
    }
    names
}

// --- PDF: version byte + exact page count only when the scan reaches EOF ---

fn parse_pdf(path: &Path, len: u64) -> Option<Vec<MetaPair>> {
    let scan = read_head(path, len.min(PDF_SCAN)).ok()?;
    let mut pairs: Vec<MetaPair> = Vec::new();
    if scan.starts_with(b"%PDF-") && scan.len() >= 8 {
        let ver = String::from_utf8_lossy(&scan[5..8]).trim().to_string();
        if !ver.is_empty() && ver.chars().all(|c| c.is_ascii_digit() || c == '.') {
            pairs.push(pair("format", format!("pdf {ver}")));
        }
    }
    if len <= PDF_SCAN {
        // Whole file inside the bounded scan → the marker count is exact.
        let pages = count_pdf_pages(&scan);
        if pages > 0 {
            pairs.push(pair("pages", pages.to_string()));
        }
    }
    Some(pairs)
}

fn count_pdf_pages(buf: &[u8]) -> usize {
    let mut count = 0usize;
    let mut rest = buf;
    while let Some(pos) = rest.windows(5).position(|w| w == b"/Type") {
        rest = &rest[pos + 5..];
        let after: &[u8] = rest;
        let after = after
            .iter()
            .skip_while(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            .copied()
            .collect::<Vec<u8>>();
        // `/Type /Page` (not `/Pages`) — skip the slash, require "Page" + boundary.
        if after.len() >= 5 && after[0] == b'/' && &after[1..5] == b"Page" {
            let boundary_ok = after.len() == 5 || !after[5].is_ascii_alphanumeric();
            if boundary_ok {
                count += 1;
            }
        }
    }
    count
}

// --- OLE compound files (msi, old doc/xls, pst) ---

fn parse_cfb(path: &Path, head: &[u8]) -> Option<Vec<MetaPair>> {
    if head.len() < 52 {
        return None;
    }
    let minor = u16le(head, 24)?;
    let major = u16le(head, 26)?;
    let sector_shift = u16le(head, 30)?;
    let mini_shift = u16le(head, 32)?;
    if sector_shift >= 64 || mini_shift >= 64 {
        return None; // malformed header — don't panic on the shift
    }
    let fat_sectors = u32le(head, 44)?;
    let first_dir = u32le(head, 48)?;

    let mut pairs = vec![
        pair("format", format!("ole compound v{major}.{minor}")),
        pair("sector size", fmt_bytes(1u64 << sector_shift)),
        pair("mini sector size", fmt_bytes(1u64 << mini_shift)),
        pair("fat sectors", fat_sectors.to_string()),
    ];
    // Stream names from the directory's first sector — real UTF-16LE bytes.
    let sector_size = 1u64 << sector_shift;
    if sector_size <= 4096 {
        if let Ok(dir_data) = read_at(path, u64::from(first_dir) * sector_size, 8192) {
            let mut names: Vec<String> = Vec::new();
            for entry in dir_data.chunks(128).take(64) {
                if entry.len() < 68 {
                    break;
                }
                let name_len = usize::from(u16le(entry, 64).unwrap_or(0));
                let entry_type = entry[66];
                if name_len < 2 || name_len > 64 || entry_type == 0 {
                    continue;
                }
                let text = decode_utf16(&entry[..name_len.min(64)], true);
                let text = text.trim_matches('\0').trim().to_string();
                if !text.is_empty() {
                    names.push(text);
                }
            }
            if !names.is_empty() {
                names.truncate(6);
                let total = names.len();
                pairs.push(pair("streams", format!("{total}: {}", names.join(", "))));
            }
        }
    }
    Some(pairs)
}

// --- Fonts: sfnt directory + 'name' table family; woff/woff2/ttc variants ---

fn sfnt_flavor_name(flavor: &[u8]) -> &'static str {
    match flavor {
        b"OTTO" => "OpenType CFF",
        b"true" => "TrueType",
        b"typ1" => "Type 1 sfnt",
        _ => "TrueType",
    }
}

fn parse_font(path: &Path, head: &[u8]) -> Option<Vec<MetaPair>> {
    if head.len() < 16 {
        return None;
    }
    let flavor = &head[0..4];
    let mut pairs: Vec<MetaPair> = Vec::new();

    if flavor == b"wOFF" {
        // WOFF1 header: 44 bytes; plain (uncompressed) 20-byte table directory.
        let num_tables = usize::from(u16le(head, 12)?);
        pairs.push(pair("format", "woff".to_string()));
        pairs.push(pair("sfnt", sfnt_flavor_name(&head[4..8]).to_string()));
        let mut tags: Vec<String> = Vec::new();
        let mut name_loc: Option<(u64, u64, u64)> = None; // (offset, comp, orig)
        for i in 0..num_tables.min(64) {
            let rec = match head.get(44 + i * 20..44 + i * 20 + 20) {
                Some(r) => r,
                None => break,
            };
            let tag = String::from_utf8_lossy(&rec[0..4]).trim().to_string();
            let off = u32le(rec, 4)?;
            let comp = u32le(rec, 8)?;
            let orig = u32le(rec, 12)?;
            if tag == "name" {
                name_loc = Some((u64::from(off), u64::from(comp), u64::from(orig)));
            }
            if tags.len() < 12 && !tag.is_empty() {
                tags.push(tag);
            }
        }
        pairs.push(pair(
            "tables",
            format!("{num_tables} ({})", tags.join(" ")),
        ));
        if let Some((off, comp, orig)) = name_loc {
            if comp == orig {
                // Uncompressed name table — readable without a zlib decoder.
                if let Some(family) = read_name_table_family(path, off, orig) {
                    pairs.push(pair("family", family));
                }
            }
        }
        return Some(pairs);
    }

    if flavor == b"wOF2" {
        // WOFF2 directory is transformed (Brotli payload) — report what is real.
        let num_tables = u16le(head, 12)?;
        pairs.push(pair("format", "woff2".to_string()));
        pairs.push(pair("sfnt", sfnt_flavor_name(&head[4..8]).to_string()));
        pairs.push(pair("tables", num_tables.to_string()));
        return Some(pairs);
    }

    let base = if flavor == b"ttcf" {
        let num_fonts = u32le(head, 8)?;
        let first = u64::from(u32le(head, 12)?);
        pairs.push(pair("format", format!("ttc ({num_fonts} fonts)")));
        first
    } else {
        pairs.push(pair("format", sfnt_flavor_name(flavor).to_string()));
        0
    };

    // Table directory at `base` (plain sfnt / first font of a collection).
    let dir: Vec<u8> = if base == 0 {
        head.to_vec()
    } else {
        read_at(path, base, TABLE_READ_CAP).ok()? // fonts > 64 KB: one bounded read
    };
    if dir.len() < 12 {
        return Some(pairs);
    }
    let num_tables = usize::from(u16le(&dir, 4).unwrap_or(0));
    if num_tables == 0 || num_tables > 128 {
        return Some(pairs);
    }
    let mut tags: Vec<String> = Vec::new();
    let mut name_loc: Option<(u64, u64)> = None;
    for i in 0..num_tables {
        let rec = match dir.get(12 + i * 16..12 + i * 16 + 16) {
            Some(r) => r,
            None => break,
        };
        let tag = String::from_utf8_lossy(&rec[0..4]).to_string();
        let off = u32le(rec, 8)?;
        let size = u32le(rec, 12)?;
        if tag == "name" {
            name_loc = Some((u64::from(off), u64::from(size)));
        }
        if tags.len() < 12 {
            tags.push(tag);
        }
    }
    pairs.push(pair(
        "tables",
        format!("{num_tables} ({})", tags.join(" ")),
    ));
    if let Some((off, size)) = name_loc {
        if let Some(family) = read_name_table_family(path, off, size) {
            pairs.push(pair("family", family));
        }
    }
    Some(pairs)
}

/// Reads the `name` table (one bounded read) and picks the best family/full
/// name record: nameID 1 beats 4, Unicode/Windows platforms beat MacRoman.
fn read_name_table_family(path: &Path, off: u64, size: u64) -> Option<String> {
    let buf = read_at(path, off, size.min(16 * 1024)).ok()?;
    if buf.len() < 6 {
        return None;
    }
    let count = usize::from(u16le(&buf, 2)?);
    let string_off = usize::from(u16le(&buf, 4)?);
    let mut best: Option<(u8, String)> = None;
    for i in 0..count.min(256) {
        let rec = buf.get(6 + i * 12..6 + i * 12 + 12)?;
        let platform = u16le(rec, 0)?;
        let name_id = u16le(rec, 4)?;
        let len = usize::from(u16le(rec, 6)?);
        let rec_off = usize::from(u16le(rec, 8)?);
        if name_id != 1 && name_id != 4 {
            continue; // 1 = family, 4 = full name
        }
        let bytes = buf.get(string_off + rec_off..string_off + rec_off + len)?;
        let text = if platform == 3 || platform == 0 {
            decode_utf16(bytes, false) // Windows/Unicode records are UTF-16BE
        } else {
            String::from_utf8_lossy(bytes).into_owned() // MacRoman ≈ ASCII
        };
        let text = scrub_line(&text).trim().to_string();
        if text.is_empty() {
            continue;
        }
        let prio = if name_id == 1 {
            if platform == 3 || platform == 0 { 0 } else { 1 }
        } else if platform == 3 || platform == 0 {
            2
        } else {
            3
        };
        if best.as_ref().is_none_or(|(p, _)| prio < *p) {
            best = Some((prio, text));
        }
    }
    best.map(|(_, name)| name)
}

// --- Compressed / audio / video containers (§A: META, no decode) ---

/// Parsed media facts: display pairs + machine values for the MEDIA card.
#[derive(Default)]
struct MediaFacts {
    pairs: Vec<MetaPair>,
    duration_ms: Option<u64>,
    w: Option<u32>,
    h: Option<u32>,
}

/// Duration sanity: header math on corrupt bytes can yield astronomically
/// large values; past ~100 years that is malformed data, not a duration.
fn sane_ms(ms: f64) -> Option<u64> {
    if ms.is_finite() && ms >= 0.0 && ms <= 315_576_000_000_000.0 {
        Some(ms as u64)
    } else {
        None
    }
}

fn parse_gzip(path: &Path, len: u64, head: &[u8]) -> Vec<MetaPair> {
    let mut pairs: Vec<MetaPair> = Vec::new();
    if head.len() < 10 {
        return pairs;
    }
    let method = match head[2] {
        8 => "deflate".to_string(),
        n => format!("method {n}"),
    };
    pairs.push(pair("format", format!("gzip ({method})")));
    let mtime = u32le(head, 4).unwrap_or(0);
    if mtime != 0 {
        pairs.push(pair("modified", fmt_unix_utc(u64::from(mtime))));
    }
    // ISIZE: last 4 bytes, LE, uncompressed size mod 2^32 (gzip spec).
    if len >= 4 {
        if let Ok(tail) = read_at(path, len - 4, 4) {
            if tail.len() == 4 {
                if let Some(isize) = u32le(&tail, 0) {
                    pairs.push(pair("uncompressed", fmt_bytes(u64::from(isize))));
                }
            }
        }
    }
    pairs
}

fn parse_wav(head: &[u8]) -> MediaFacts {
    let mut pairs: Vec<MetaPair> = Vec::new();
    let mut duration_ms = None;
    pairs.push(pair("format", "wav".to_string()));
    let mut pos = 12usize;
    let mut byte_rate: u32 = 0;
    let mut data_size: Option<u64> = None;
    while pos + 8 <= head.len() {
        let id = &head[pos..pos + 4];
        let size = u32le(head, pos + 4).unwrap_or(0) as usize;
        let body = pos + 8;
        if id == b"fmt " && head.len() >= body + 16 {
            let codec = u16le(head, body).unwrap_or(0);
            let channels = u16le(head, body + 2).unwrap_or(0);
            let rate = u32le(head, body + 4).unwrap_or(0);
            byte_rate = u32le(head, body + 8).unwrap_or(0);
            let bits = u16le(head, body + 14).unwrap_or(0);
            let codec_name = match codec {
                1 => "pcm".to_string(),
                3 => "pcm float".to_string(),
                0xFFFE => "extensible".to_string(),
                n => format!("0x{n:04x}"),
            };
            pairs.push(pair("codec", codec_name));
            pairs.push(pair("channels", channels.to_string()));
            pairs.push(pair("sample rate", format!("{rate} Hz")));
            pairs.push(pair("bit depth", bits.to_string()));
        } else if id == b"data" {
            data_size = Some(size as u64);
            break;
        }
        let next = body + size + (size & 1); // chunks are word-aligned
        if next <= pos {
            break;
        }
        pos = next;
    }
    if let Some(ds) = data_size {
        if byte_rate > 0 {
            let secs = ds as f64 / f64::from(byte_rate);
            pairs.push(pair("duration", format!("{secs:.1} s")));
            duration_ms = sane_ms(secs * 1000.0);
        }
    }
    MediaFacts {
        pairs,
        duration_ms,
        ..Default::default()
    }
}

fn parse_flac(head: &[u8]) -> MediaFacts {
    let mut pairs: Vec<MetaPair> = Vec::new();
    // "fLaC" (4) + block header (4) + STREAMINFO (34); packed fields at 18..26.
    if head.len() < 26 {
        return MediaFacts::default();
    }
    let mut duration_ms = None;
    pairs.push(pair("format", "flac".to_string()));
    let b = &head[18..26];
    let rate = (u32::from(b[0]) << 12) | (u32::from(b[1]) << 4) | (u32::from(b[2]) >> 4);
    let channels = ((b[2] >> 1) & 0b111) + 1;
    let bits = (((b[2] & 1) << 4) | (b[3] >> 4)) + 1;
    let total_samples = ((u64::from(b[3]) & 0x0F) << 32)
        | u64::from(u32::from_be_bytes([b[4], b[5], b[6], b[7]]));
    if rate > 0 {
        pairs.push(pair("sample rate", format!("{rate} Hz")));
        pairs.push(pair("channels", channels.to_string()));
        pairs.push(pair("bit depth", bits.to_string()));
        if total_samples > 0 {
            let secs = total_samples as f64 / f64::from(rate);
            pairs.push(pair("duration", format!("{secs:.1} s")));
            duration_ms = sane_ms(secs * 1000.0);
        }
    }
    MediaFacts {
        pairs,
        duration_ms,
        ..Default::default()
    }
}

fn parse_ogg(head: &[u8]) -> MediaFacts {
    let mut pairs: Vec<MetaPair> = vec![pair("format", "ogg".to_string())];
    let window = &head[..head.len().min(4096)];
    let codec = if window.windows(7).any(|w| w == b"\x01vorbis") {
        Some("vorbis")
    } else if window.windows(8).any(|w| w == b"OpusHead") {
        Some("opus")
    } else if window.windows(5).any(|w| w == b"\x7fFLAC") {
        Some("flac")
    } else if window.windows(5).any(|w| w == b"theora") {
        Some("theora")
    } else {
        None
    };
    if let Some(c) = codec {
        pairs.push(pair("codec", c.to_string()));
    }
    // Ogg duration needs the final page's granule position (a whole-file
    // walk) — absent here on purpose, never guessed.
    MediaFacts {
        pairs,
        ..Default::default()
    }
}

fn parse_mp3(head: &[u8]) -> MediaFacts {
    let mut pairs: Vec<MetaPair> = Vec::new();
    let mut frame_off = 0usize;
    if head.starts_with(b"ID3") && head.len() >= 10 {
        let version = head[3];
        pairs.push(pair("format", format!("mp3 (id3v2.{version})")));
        let tag_size = (u32::from(head[6] & 0x7F) << 21)
            | (u32::from(head[7] & 0x7F) << 14)
            | (u32::from(head[8] & 0x7F) << 7)
            | u32::from(head[9] & 0x7F); // synchsafe integer (ID3 spec)
        pairs.push(pair("id3 tag", fmt_bytes(u64::from(tag_size))));
        frame_off = 10 + tag_size as usize;
    } else {
        pairs.push(pair("format", "mp3".to_string()));
    }
    // First audio frame header: sync + version + layer + sample rate + mode.
    let sync_at = if head.len() >= frame_off + 4 && head[frame_off] == 0xFF {
        Some(frame_off)
    } else {
        head[..head.len().min(4096)]
            .windows(2)
            .position(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0)
    };
    if let Some(off) = sync_at {
        if head.len() >= off + 4 {
            let b1 = head[off + 1];
            let b2 = head[off + 2];
            let b3 = head[off + 3];
            let version = match (b1 >> 3) & 0b11 {
                3 => Some(("MPEG1", [44100u32, 48000, 32000])),
                2 => Some(("MPEG2", [22050u32, 24000, 16000])),
                0 => Some(("MPEG2.5", [11025u32, 12000, 8000])),
                _ => None,
            };
            let layer = match (b1 >> 1) & 0b11 {
                1 => Some("layer III"),
                2 => Some("layer II"),
                3 => Some("layer I"),
                _ => None,
            };
            if let (Some((ver, rates)), Some(layer)) = (version, layer) {
                let rate = usize::from((b2 >> 2) & 0b11);
                if let Some(rate) = rates.get(rate) {
                    pairs.push(pair("mpeg", format!("{ver} {layer}")));
                    pairs.push(pair("sample rate", format!("{rate} Hz")));
                    let mode = match b3 >> 6 {
                        0 => "joint stereo",
                        1 => "stereo",
                        2 => "dual channel",
                        _ => "mono",
                    };
                    pairs.push(pair("channels", mode.to_string()));
                }
            }
        }
    }
    // MP3 duration needs bitrate integration across frames (VBR breaks any
    // cheap estimate) — absent here on purpose, never guessed.
    MediaFacts {
        pairs,
        ..Default::default()
    }
}

fn parse_mp4(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    if head.len() >= 16 && &head[4..8] == b"ftyp" {
        let brand = ascii4(&head[8..12]);
        facts
            .pairs
            .push(pair("format", format!("mp4 ftyp {brand}")));
        let box_size = (u32be(head, 0).unwrap_or(16) as usize).min(head.len());
        let mut compatible: Vec<String> = Vec::new();
        let mut pos = 16usize;
        while pos + 4 <= box_size {
            let b = ascii4(&head[pos..pos + 4]);
            if !b.is_empty() {
                compatible.push(b);
            }
            pos += 4;
            if compatible.len() >= 6 {
                break;
            }
        }
        if !compatible.is_empty() {
            facts.pairs.push(pair("compatible", compatible.join(" ")));
        }
    }
    mp4_header_facts(head, &mut facts);
    facts
}

/// mvhd (timescale → duration) and tkhd (16.16 fixed → pixel size) inside a
/// moov-shaped buffer. Bounds-checked window scans; nothing is assumed.
fn mp4_header_facts(buf: &[u8], facts: &mut MediaFacts) {
    if facts.duration_ms.is_none() {
        if let Some(idx) = buf.windows(4).position(|w| w == b"mvhd") {
            let p = idx + 4;
            let version = *buf.get(p).unwrap_or(&0);
            // v0: ts@+12 dur@+16; v1: ts@+20 dur@+24 (64-bit).
            let (ts_off, dur_off, wide) = if version == 1 {
                (20usize, 24usize, true)
            } else {
                (12usize, 16usize, false)
            };
            if let Some(timescale) = u32be(buf, p + ts_off) {
                let duration = if wide {
                    buf.get(p + dur_off..p + dur_off + 8).and_then(|s| {
                        Some((u64::from(u32be(s, 0)?) << 32) | u64::from(u32be(s, 4)?))
                    })
                } else {
                    u32be(buf, p + dur_off).map(u64::from)
                };
                if let Some(duration) = duration {
                    if timescale > 0 && duration > 0 {
                        let secs = duration as f64 / f64::from(timescale);
                        if let Some(ms) = sane_ms(secs * 1000.0) {
                            facts.duration_ms = Some(ms);
                            facts.pairs.push(pair("duration", format!("{secs:.1} s")));
                        }
                    }
                }
            }
        }
    }
    if facts.w.is_none() {
        if let Some(idx) = buf.windows(4).position(|w| w == b"tkhd") {
            let p = idx + 4;
            let version = *buf.get(p).unwrap_or(&0);
            // Fixed32 width/height close the box: v0 @76/80, v1 @84/88.
            let (w_off, h_off) = if version == 1 {
                (84usize, 88usize)
            } else {
                (76usize, 80usize)
            };
            let w = buf.get(p + w_off..p + w_off + 4).and_then(|s| u32be(s, 0));
            let h = buf.get(p + h_off..p + h_off + 4).and_then(|s| u32be(s, 0));
            if let (Some(w), Some(h)) = (w, h) {
                // 16.16 fixed point — only whole, non-zero pixels.
                if w >= 0x1_0000 && h >= 0x1_0000 {
                    facts.w = Some(w >> 16);
                    facts.h = Some(h >> 16);
                }
            }
        }
    }
}

/// Non-faststart mp4-family files keep `moov` at the END. One bounded
/// top-level box walk (≤32 boxes, ≤256 KB of moov — mvhd/tkhd sit at its
/// head) locates it without ever reading the media data.
fn mp4_moov_head(path: &Path, len: u64) -> Vec<u8> {
    const BOXES: u32 = 32;
    const MOOV_CAP: u64 = 256 * 1024;
    let mut off = 0u64;
    for _ in 0..BOXES {
        if off + 16 > len {
            break;
        }
        let hdr = match read_at(path, off, 16) {
            Ok(h) if h.len() == 16 => h,
            _ => break,
        };
        let mut size = u64::from(u32be(&hdr, 0).unwrap_or(0));
        let kind = &hdr[4..8];
        let mut body = off + 8;
        if size == 1 {
            // 64-bit extent: the wide size follows the type.
            let wide: [u8; 8] = match read_at(path, off + 8, 8)
                .ok()
                .and_then(|w| w.try_into().ok())
            {
                Some(w) => w,
                None => break,
            };
            size = u64::from_be_bytes(wide);
            body = off + 16;
        } else if size == 0 {
            size = len.saturating_sub(off); // box runs to EOF
        }
        if size < 8 {
            break; // malformed — never chase a shrinking box
        }
        if kind == b"moov" {
            return read_at(path, body, size.min(MOOV_CAP)).unwrap_or_default();
        }
        let next = off.saturating_add(size);
        if next <= off || next >= len {
            break;
        }
        off = next;
    }
    Vec::new()
}

// --- Matroska/WebM (EBML): duration + first video track size, bounded ---

/// One EBML element header: `(id, data_size, total_header_len, unknown_size)`.
/// `None` on truncation; the unknown-size marker is flagged, never chased
/// past the buffer.
fn ebml_element(buf: &[u8], pos: usize) -> Option<(u64, u64, usize, bool)> {
    let (id, id_len) = ebml_vint(buf, pos)?; // raw bytes: the marker bits ARE the id
    let (size, size_len, unknown) = ebml_size(buf, pos + id_len)?;
    Some((id, size, id_len + size_len, unknown))
}

/// Reads an EBML VINT at `pos`: the raw big-endian bytes (keeping the
/// length-descriptor bit, so IDs compare directly) and the byte length.
fn ebml_vint(buf: &[u8], pos: usize) -> Option<(u64, usize)> {
    let first = *buf.get(pos)?;
    if first == 0 {
        return None; // the descriptor would need a 9th byte
    }
    let len = first.leading_zeros() as usize + 1;
    if len > 8 {
        return None;
    }
    let mut raw = 0u64;
    for i in 0..len {
        raw = (raw << 8) | u64::from(*buf.get(pos + i)?);
    }
    Some((raw, len))
}

/// The SIZE half: strips the descriptor bit; flags the all-ones unknown-size
/// marker. VINT widths cap sizes at 2^56−1 by construction.
fn ebml_size(buf: &[u8], pos: usize) -> Option<(u64, usize, bool)> {
    let (raw, len) = ebml_vint(buf, pos)?;
    let value = raw & !(1u64 << (7 * len));
    let unknown = value == (1u64 << (7 * len)) - 1;
    Some((value, len, unknown))
}

/// Bounded direct-children scan of one EBML container `[start, end)`. Never
/// reads outside the buffer, enforces forward progress, caps iterations.
fn ebml_scan<F: FnMut(u64, usize, usize)>(buf: &[u8], start: usize, end: usize, mut f: F) {
    let mut pos = start.min(end);
    for _ in 0..256 {
        if pos >= end {
            break;
        }
        let Some((id, size, hdr, unknown)) = ebml_element(buf, pos) else {
            break;
        };
        let data = pos + hdr; // hdr ≤ 16, pos < end ≤ 64 KB — no overflow
        if data > end {
            break;
        }
        let elem_end = if unknown {
            end
        } else {
            data.saturating_add(usize::try_from(size).unwrap_or(usize::MAX))
                .min(end)
        };
        f(id, data, elem_end);
        if elem_end <= pos {
            break; // degenerate size — refuse to spin
        }
        pos = elem_end;
    }
}

fn ebml_uint(buf: &[u8], start: usize, end: usize) -> Option<u64> {
    let n = end.saturating_sub(start).min(8);
    if n == 0 {
        return None;
    }
    let s = buf.get(start..start + n)?;
    let mut v = 0u64;
    for &b in s {
        v = (v << 8) | u64::from(b);
    }
    Some(v)
}

fn ebml_float(buf: &[u8], start: usize, end: usize) -> Option<f64> {
    let n = end.saturating_sub(start);
    let s = buf.get(start..start + n)?;
    match n {
        4 => Some(f64::from(f32::from_be_bytes(s.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(s.try_into().ok()?)),
        _ => None,
    }
}

fn parse_mkv(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    // DocType is the EBML header's first child — always within the first ~64 B.
    let window = &head[..head.len().min(1024)];
    let format = if window.windows(4).any(|w| w == b"webm") {
        "webm"
    } else if window.windows(8).any(|w| w == b"matroska") {
        "matroska"
    } else {
        "ebml"
    };
    facts.pairs.push(pair("format", format.to_string()));

    // Segment (usually the 2nd top-level element) bounds everything below.
    let mut segment: Option<(usize, usize)> = None;
    let mut start = 0usize;
    for _ in 0..8 {
        let Some((id, size, hdr, unknown)) = ebml_element(head, start) else {
            break;
        };
        let data = start + hdr;
        let end = if unknown {
            head.len()
        } else {
            data.saturating_add(usize::try_from(size).unwrap_or(usize::MAX))
                .min(head.len())
        };
        if id == 0x1853_8067 {
            segment = Some((data, end));
            break;
        }
        if end <= start {
            break;
        }
        start = end;
    }
    let Some((seg_start, seg_end)) = segment else {
        return facts;
    };

    let mut scale_ns = 1_000_000u64; // EBML default: nanoseconds per tick
    let mut ticks: Option<f64> = None;
    ebml_scan(head, seg_start, seg_end, |id, data, end| match id {
        0x1549_A966 => {
            // Info: TimecodeScale + Duration.
            ebml_scan(head, data, end, |iid, idata, iend| match iid {
                0x2AD7_B1 => {
                    scale_ns = ebml_uint(head, idata, iend)
                        .filter(|v| *v > 0)
                        .unwrap_or(scale_ns)
                }
                0x4489 => ticks = ebml_float(head, idata, iend),
                _ => {}
            });
        }
        0x1654_AE6B => {
            // Tracks → first video TrackEntry → pixel size.
            ebml_scan(head, data, end, |tid, tdata, tend| {
                if tid != 0xAE {
                    return; // TrackEntry
                }
                let mut video = false;
                ebml_scan(head, tdata, tend, |cid, cdata, cend| match cid {
                    0x83 => video = ebml_uint(head, cdata, cend) == Some(1),
                    0xE0 if video => {
                        // Video: PixelWidth / PixelHeight.
                        ebml_scan(head, cdata, cend, |vid, vdata, vend| match vid {
                            0xB0 => {
                                facts.w = ebml_uint(head, vdata, vend)
                                    .and_then(|v| u32::try_from(v).ok())
                            }
                            0xBA => {
                                facts.h = ebml_uint(head, vdata, vend)
                                    .and_then(|v| u32::try_from(v).ok())
                            }
                            _ => {}
                        })
                    }
                    _ => {}
                });
            });
        }
        _ => {}
    });

    if let Some(t) = ticks {
        facts.duration_ms = sane_ms(t * scale_ns as f64 / 1_000_000.0);
        if let Some(ms) = facts.duration_ms {
            facts
                .pairs
                .push(pair("duration", format!("{:.1} s", ms as f64 / 1000.0)));
        }
    }
    facts
}

fn parse_avi(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    facts.pairs.push(pair("format", "avi".to_string()));
    let Some(idx) = head.windows(4).position(|w| w == b"avih") else {
        return facts;
    };
    // 'avih' main header: 4-byte size precedes the type; the body follows.
    let body = idx + 8;
    let micro = u32le(head, body).unwrap_or(0); // microseconds per frame
    let frames = u32le(head, body + 16).unwrap_or(0);
    let w = u32le(head, body + 32).unwrap_or(0);
    let h = u32le(head, body + 36).unwrap_or(0);
    if micro > 0 && frames > 0 {
        facts.duration_ms = sane_ms(f64::from(micro) * f64::from(frames) / 1000.0);
        if let Some(ms) = facts.duration_ms {
            facts
                .pairs
                .push(pair("duration", format!("{:.1} s", ms as f64 / 1000.0)));
        }
    }
    if w > 0 && h > 0 {
        facts.w = Some(w);
        facts.h = Some(h);
    }
    facts
}

fn parse_mobi(head: &[u8]) -> Vec<MetaPair> {
    let mut pairs: Vec<MetaPair> = Vec::new();
    let is_mobi = head.len() >= 68 && &head[60..68] == b"BOOKMOBI";
    let is_tead = head.len() >= 68 && &head[60..68] == b"TEXtREAd";
    if !is_mobi && !is_tead {
        return pairs;
    }
    pairs.push(pair(
        "format",
        if is_mobi { "mobi" } else { "palm doc" }.to_string(),
    ));
    // PalmDB header (Palm spec): title at 0..32, modification time at 40.
    let title = cstr_field(&head[..32.min(head.len())]);
    if !title.is_empty() {
        pairs.push(pair("title", title));
    }
    if let Some(palm_time) = u32le(head, 40) {
        // Palm epoch = 1904-01-01; seconds since Unix epoch = palm − 2 082 844 800.
        if palm_time > 2_082_844_800 {
            pairs.push(pair(
                "modified",
                fmt_unix_utc(u64::from(palm_time) - 2_082_844_800),
            ));
        }
    }
    pairs
}

fn parse_ar(head: &[u8]) -> Vec<MetaPair> {
    let mut pairs: Vec<MetaPair> = vec![pair("format", "ar".to_string())];
    let mut names: Vec<String> = Vec::new();
    let mut pos = 8usize;
    while pos + 60 <= head.len() && names.len() < 12 {
        if &head[pos + 58..pos + 60] != b"\x60\x0a" {
            break; // per-member magic `` + LF ends a valid ar header
        }
        let name = cstr_field(&head[pos..pos + 16]).trim_end_matches('/').to_string();
        let size_field = &head[pos + 48..pos + 58];
        let size_text = String::from_utf8_lossy(size_field)
            .trim()
            .trim_matches('\0')
            .to_string();
        let size: u64 = std::str::from_utf8(size_text.as_bytes())
            .ok()
            .and_then(|s| u64::from_str_radix(s, 8).ok())
            .unwrap_or(0);
        if !name.is_empty() {
            names.push(name);
        }
        let next = pos + 60 + size as usize + (size as usize & 1); // even-aligned
        if next <= pos {
            break;
        }
        pos = next;
    }
    if !names.is_empty() {
        let total = names.len();
        names.truncate(4);
        pairs.push(pair("members", format!("{total}: {}", names.join(", "))));
    }
    pairs
}

fn parse_cab(head: &[u8]) -> Vec<MetaPair> {
    let mut pairs: Vec<MetaPair> = Vec::new();
    if head.len() < 28 {
        return pairs;
    }
    // CFHEADER (MS cab spec): cbCabinet@8, cFolders@18, cFiles@20.
    let cb_cabinet = u32le(head, 8).unwrap_or(0);
    let folders = u16le(head, 18).unwrap_or(0);
    let files = u16le(head, 20).unwrap_or(0);
    pairs.push(pair("format", "cab".to_string()));
    pairs.push(pair("files", files.to_string()));
    pairs.push(pair("folders", folders.to_string()));
    pairs.push(pair("cabinet size", fmt_bytes(u64::from(cb_cabinet))));
    pairs
}

// --- Zip-backed counters (pptx/odp slides, jar/apk entries) ---

fn parse_slides(path: &Path, ext: &str) -> Vec<MetaPair> {
    let mut pairs: Vec<MetaPair> = Vec::new();
    let mut archive = match zip_open(path) {
        Ok(a) => a,
        Err(_) => return pairs,
    };
    if ext == "pptx" {
        pairs.push(pair("format", "pptx (zip)".to_string()));
        // Slide parts live in the central dir — counted without decompressing.
        let count = archive
            .file_names()
            .filter(|n| {
                n.starts_with("ppt/slides/slide") && n.ends_with(".xml") && !n.contains("rels")
            })
            .count();
        if count > 0 {
            pairs.push(pair("slides", count.to_string()));
        }
        return pairs;
    }
    // odp: page count from content.xml, exact only when it fits the 64 KB cap.
    pairs.push(pair("format", "odp (zip)".to_string()));
    let has_content = archive.file_names().any(|n| n == "content.xml");
    if has_content {
        if let Ok(f) = archive.by_name("content.xml") {
            let size = f.size();
            if size > 0 && size <= TEXT_CAP {
                let mut buf = Vec::new();
                if f.take(TEXT_CAP + 1).read_to_end(&mut buf).is_ok() {
                    let xml = String::from_utf8_lossy(&buf);
                    let count = xml.matches("<draw:page ").count();
                    pairs.push(pair("slides", count.to_string()));
                }
            }
        }
    }
    pairs
}

fn parse_zip_count(path: &Path) -> Vec<MetaPair> {
    let mut pairs: Vec<MetaPair> = Vec::new();
    let archive = match zip_open(path) {
        Ok(a) => a,
        Err(_) => return pairs,
    };
    pairs.push(pair("format", "zip".to_string()));
    pairs.push(pair("archive entries", archive.len().to_string()));
    pairs
}



