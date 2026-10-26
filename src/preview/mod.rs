//! Safe, bounded previews. Plain Rust types only: no serde, no data URLs, no
//! web media. File bytes are read, never executed or written.
//!
//! Routing is extension-first with a content-sniff fallback. Every family has
//! hard caps; archives are never extracted to disk, SQLite is opened read-only
//! and image decoding is limited by byte count, pixel count and allocation.

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};

mod cache;
mod db;
mod hex;
mod images;
mod list;
mod media;
mod meta;
mod text;
mod util;

use cache::{CacheKey, PreviewCache};
use util::{TEXT_CAP, at, ext_lower, read_head};

#[derive(Debug, Clone)]
pub enum Preview {
    Text(TextPreview),
    Hex(HexPreview),
    Image(ImagePreview),
    List(ListPreview),
    Meta(MetaPreview),
    Db(DbPreview),
    Media(MediaPreview),
    Unavailable { reason: String },
}

impl Preview {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "TEXT",
            Self::Hex(_) => "HEX",
            Self::Image(_) => "IMAGE",
            Self::List(_) => "LIST",
            Self::Meta(_) => "META",
            Self::Db(_) => "DB",
            Self::Media(_) => "MEDIA",
            Self::Unavailable { .. } => "UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TextPreview {
    pub lines: Vec<String>,
    pub truncated: bool,
    pub total_lines: Option<u64>,
    pub encoding: Option<String>,
    /// Language hint for the UI (`rust`, `json`, `toml`, ...).
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HexRow {
    pub offset: u64,
    pub hex: String,
    pub ascii: String,
}

#[derive(Debug, Clone)]
pub struct HexPreview {
    pub rows: Vec<HexRow>,
    pub total: u64,
}

#[derive(Debug, Clone)]
pub struct ImagePreview {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, top-left origin.
    pub rgba: Arc<[u8]>,
    pub mime: String,
    pub source_width: u32,
    pub source_height: u32,
}

#[derive(Debug, Clone)]
pub struct ListEntry {
    pub name: String,
    pub size: Option<u64>,
    pub kind: ListKind,
    pub method: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Dir,
    File,
}

#[derive(Debug, Clone)]
pub struct ListPreview {
    /// folder | zip | tar | iso | sheet | office
    pub kind: String,
    pub entries: Vec<ListEntry>,
    pub truncated: bool,
    pub total: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct MetaPair {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct MetaPreview {
    pub pairs: Vec<MetaPair>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DbTable {
    pub name: String,
    pub cols: u32,
    pub columns: Option<Vec<String>>,
    pub rows: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Clone)]
pub struct DbPreview {
    pub tables: Vec<DbTable>,
    pub truncated: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaPreview {
    /// "video" | "audio"
    pub kind: String,
    pub mime: String,
    pub thumbnail: Option<ImagePreview>,
    pub pairs: Vec<MetaPair>,
    /// Honest explanation when no decoder/thumbnail is available.
    pub note: Option<String>,
}

/// Bounded preview service, cheap to clone across worker threads.
#[derive(Clone)]
pub struct Service {
    inner: Arc<ServiceInner>,
}

struct ServiceInner {
    cache: Mutex<PreviewCache>,
    gate: Gate,
}

/// At most four preview builds run at once. Cache hits bypass the gate.
struct Gate {
    slots: Mutex<usize>,
    cv: Condvar,
}

struct GateGuard<'a> {
    gate: &'a Gate,
}

impl Gate {
    fn new(permits: usize) -> Self {
        Self {
            slots: Mutex::new(permits),
            cv: Condvar::new(),
        }
    }

    fn acquire(&self) -> GateGuard<'_> {
        let mut slots = self.slots.lock();
        while *slots == 0 {
            self.cv.wait(&mut slots);
        }
        *slots -= 1;
        GateGuard { gate: self }
    }
}

impl Drop for GateGuard<'_> {
    fn drop(&mut self) {
        let mut slots = self.gate.slots.lock();
        *slots += 1;
        self.gate.cv.notify_one();
    }
}

impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}

impl Service {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ServiceInner {
                cache: Mutex::new(PreviewCache::new()),
                gate: Gate::new(4),
            }),
        }
    }

    /// Returns a bounded preview. Never panics: malformed files, permission
    /// errors, symlinks and unsupported node types degrade to `Unavailable`
    /// or a META card carrying the reason.
    pub fn preview(&self, path: &Path) -> Preview {
        if path.as_os_str().is_empty() {
            return Preview::Unavailable {
                reason: "empty path".to_string(),
            };
        }
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) => {
                return Preview::Unavailable {
                    reason: util::io_reason(&e),
                };
            }
        };
        if meta.file_type().is_symlink() {
            return Preview::Unavailable {
                reason: "symlink (not followed)".to_string(),
            };
        }
        let is_dir = meta.is_dir();
        let is_file = meta.is_file();
        if !is_dir && !is_file {
            return Preview::Unavailable {
                reason: "not a regular file".to_string(),
            };
        }
        let key = CacheKey {
            path: path.to_path_buf(),
            len: if is_dir { 0 } else { meta.len() },
            mtime_nanos: util::mtime_nanos(&meta),
            is_dir,
        };

        if let Some(hit) = self.inner.cache.lock().get(&key) {
            return hit;
        }

        let _permit = self.inner.gate.acquire();
        if let Some(hit) = self.inner.cache.lock().get(&key) {
            return hit;
        }

        let built = panic::catch_unwind(AssertUnwindSafe(|| {
            if is_dir {
                build_dir(path, &meta)
            } else {
                build_file(path, &meta)
            }
        }))
        .unwrap_or_else(|_| Preview::Unavailable {
            reason: "preview failed on malformed input".to_string(),
        });

        if !matches!(built, Preview::Unavailable { .. }) {
            self.inner.cache.lock().insert(key, built.clone());
        }
        built
    }

    pub fn cache_len(&self) -> usize {
        self.inner.cache.lock().len()
    }
}

fn build_dir(path: &Path, meta: &fs::Metadata) -> Preview {
    match list::folder(path) {
        Ok(list) => Preview::List(list),
        Err(reason) => meta::degraded(path, Some(meta), &reason, ""),
    }
}

fn build_file(path: &Path, meta: &fs::Metadata) -> Preview {
    let len = meta.len();
    if len == 0 {
        return meta::empty(meta);
    }
    let ext = ext_lower(path);
    let mut family = family_for_ext(&ext);

    // `.ts` is both TypeScript and an MPEG transport stream. Bytes decide.
    if ext == "ts" {
        let head = read_head(path, len.min(512)).unwrap_or_default();
        if !is_mpegts(&head) {
            family = Family::Text;
        }
    }

    match family {
        Family::Text => match text::build(path, len, ext == "svg") {
            Ok(Some(mut preview)) => {
                preview.language = text::language(&ext);
                Preview::Text(preview)
            }
            Ok(None) => hex_or_meta(path, meta, &ext),
            Err(reason) => meta::degraded(path, Some(meta), &reason, &ext),
        },
        Family::Hex => hex_or_meta(path, meta, &ext),
        Family::Image => match images::build(path, len) {
            Some(image) => Preview::Image(image),
            None => meta::build(path, meta, &ext),
        },
        Family::Db => {
            let head = read_head(path, len.min(16)).unwrap_or_default();
            if head.starts_with(b"SQLite format 3\x00") {
                match db::build(path) {
                    Ok(db) => Preview::Db(db),
                    Err(_) => meta::build(path, meta, &ext),
                }
            } else {
                sniff_unknown(path, meta, &ext)
            }
        }
        Family::List => match ext.as_str() {
            "tar" => list::tar(path, len)
                .map(Preview::List)
                .unwrap_or_else(|_| meta::build(path, meta, &ext)),
            "iso" | "img" => list::iso(path, len)
                .map(Preview::List)
                .unwrap_or_else(|_| meta::build(path, meta, &ext)),
            "zip" | "epub" | "pptx" | "odp" => list::zip(path)
                .map(Preview::List)
                .unwrap_or_else(|_| meta::build(path, meta, &ext)),
            _ => list::zip(path)
                .map(Preview::List)
                .unwrap_or_else(|_| meta::build(path, meta, &ext)),
        },
        Family::Sheet => list::sheet(path, &ext)
            .or_else(|_| list::zip(path))
            .map(Preview::List)
            .unwrap_or_else(|_| meta::build(path, meta, &ext)),
        Family::Office => list::office(path, &ext)
            .or_else(|_| list::zip(path))
            .map(Preview::List)
            .unwrap_or_else(|_| meta::build(path, meta, &ext)),
        Family::Media => Preview::Media(media::build(path, &ext, len)),
        Family::Meta => meta::build(path, meta, &ext),
        Family::Sniff => sniff_unknown(path, meta, &ext),
    }
}

fn hex_or_meta(path: &Path, meta: &fs::Metadata, ext: &str) -> Preview {
    match hex::build(path, meta.len()) {
        Ok(hex) => Preview::Hex(hex),
        Err(reason) => meta::degraded(path, Some(meta), &reason, ext),
    }
}

/// Content sniffing for unknown extensions and for known-but-mismatched ones.
fn sniff_unknown(path: &Path, meta: &fs::Metadata, ext: &str) -> Preview {
    let len = meta.len();
    let head = match read_head(path, len.min(TEXT_CAP)) {
        Ok(h) => h,
        Err(reason) => return meta::degraded(path, Some(meta), &reason, ext),
    };

    if head.starts_with(b"SQLite format 3\x00") {
        if let Ok(db) = db::build(path) {
            return Preview::Db(db);
        }
        return meta::build_from_head(path, meta, ext, &head, None);
    }
    if head.starts_with(b"PK\x03\x04")
        || head.starts_with(b"PK\x05\x06")
        || head.starts_with(b"PK\x07\x08")
    {
        if let Ok(list) = list::zip(path) {
            return Preview::List(list);
        }
        return meta::build_from_head(path, meta, ext, &head, None);
    }
    if at(&head, 257, b"ustar")
        && let Ok(list) = list::tar(path, len)
    {
        return Preview::List(list);
    }
    if at(&head, 0x8001, b"CD001")
        && let Ok(list) = list::iso(path, len)
    {
        return Preview::List(list);
    }
    if meta::image_signature(&head) {
        if let Some(image) = images::thumb_from_bytes(&head) {
            return Preview::Image(image);
        }
        return meta::build_from_head(path, meta, ext, &head, None);
    }
    if meta::sig_format(&head).is_some() {
        return meta::build_from_head(path, meta, ext, &head, None);
    }

    match text::classify_text(&head) {
        text::TextClass::Binary => {
            Preview::Hex(hex::from_bytes(&head[..head.len().min(4096)], len))
        }
        _ => match text::from_head(&head, len, ext == "svg") {
            Some(mut preview) => {
                preview.language = text::language(ext);
                Preview::Text(preview)
            }
            None => Preview::Hex(hex::from_bytes(&head[..head.len().min(4096)], len)),
        },
    }
}

fn is_mpegts(head: &[u8]) -> bool {
    head.len() >= 189 && head[0] == 0x47 && head[188] == 0x47
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Text,
    Hex,
    Image,
    List,
    Sheet,
    Office,
    Db,
    Meta,
    Media,
    Sniff,
}

const TEXT_EXTS: &[&str] = &[
    "txt",
    "md",
    "rtf",
    "log",
    "out",
    "rs",
    "tsx",
    "jsx",
    "mjs",
    "cjs",
    "py",
    "java",
    "c",
    "cpp",
    "cc",
    "cxx",
    "h",
    "hpp",
    "hxx",
    "cs",
    "go",
    "sh",
    "bash",
    "zsh",
    "ps1",
    "bat",
    "cmd",
    "sql",
    "vue",
    "svelte",
    "rb",
    "php",
    "pl",
    "pm",
    "swift",
    "kt",
    "kts",
    "scala",
    "lua",
    "r",
    "jl",
    "dart",
    "ex",
    "exs",
    "erl",
    "hs",
    "ml",
    "asm",
    "s",
    "json",
    "yaml",
    "yml",
    "toml",
    "xml",
    "csv",
    "ini",
    "cfg",
    "conf",
    "env",
    "properties",
    "reg",
    "plist",
    "obj",
    "stl",
    "ply",
    "gltf",
    "dxf",
    "step",
    "iges",
    "stp",
    "pem",
    "crt",
    "cer",
    "asc",
    "key",
    "pub",
    "eml",
    "mbox",
    "srt",
    "ass",
    "ssa",
    "vtt",
    "svg",
    "eps",
    "html",
    "htm",
    "css",
    "scss",
    "mts",
    "cts",
];

const HEX_EXTS: &[&str] = &[
    "bin",
    "dat",
    "dmp",
    "pak",
    "mdb",
    "accdb",
    "tmp",
    "temp",
    "part",
    "crdownload",
    "idx",
    "sub",
    "fbx",
    "blend",
    "dwg",
    "glb",
];

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif", "ico", "heic",
];

const DB_EXTS: &[&str] = &["db", "sqlite", "sqlite3"];
const LIST_EXTS: &[&str] = &["zip", "epub", "tar", "iso", "img", "pptx", "odp"];
const SHEET_EXTS: &[&str] = &["xlsx", "ods"];
const OFFICE_EXTS: &[&str] = &["docx", "odt"];
const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v", "ts", "m2ts"];
const AUDIO_EXTS: &[&str] = &[
    "mp3", "wav", "flac", "m4a", "aac", "ogg", "oga", "opus", "wma",
];
const META_EXTS: &[&str] = &[
    "pdf", "mobi", "gz", "tgz", "bz2", "xz", "zst", "br", "lz4", "exe", "dll", "msi", "so",
    "dylib", "deb", "rpm", "apk", "jar", "efi", "sys", "drv", "fw", "rom", "fd", "ai", "der",
    "pfx", "p12", "jks", "msg", "pst", "ost", "bak", "backup", "bkp", "vhd", "vhdx", "wim", "cab",
    "ttf", "otf", "ttc", "woff", "woff2", "fon",
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
        Family::Sheet
    } else if OFFICE_EXTS.contains(&ext) {
        Family::Office
    } else if META_EXTS.contains(&ext) {
        Family::Meta
    } else {
        Family::Sniff
    }
}
