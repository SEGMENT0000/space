//! On-tile art: thumbnails, text snippets and factual previews for large
//! treemap tiles.
//!
//! Same safety contract as the inspector previews: files are opened read-only,
//! never executed, never written. Hard limits apply before any read: an image
//! larger than [`images::IMAGE_BYTES_CAP`] is rejected from its length alone,
//! text reads a bounded [`TILE_TEXT_HEAD`] head, and archives/databases use the
//! same bounded parsers as the inspector. Unsupported or malformed input
//! returns `None` so the UI falls back to a bundled icon.
//!
//! The opt-in "Rich previews (uses Windows)" flag only adds a call to
//! [`super::winthumb`] for families whose real content the in-process decoders
//! cannot render (PDF, video, Office, fonts, unsupported image codecs). That
//! call runs on the tile worker, has its own hard timeout and byte cap, and
//! never executes the file.

use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use super::cache::Lru;
use super::util::{fmt_bytes, mtime_nanos, read_head, scrub_inline};
use super::{Gate, db, hex, images, list, media, meta, text};
use crate::model::CategoryTotals;

/// Target thumbnail edge requested per tile size bucket. Never above the
/// decoder's own 512 px cap.
pub const TILE_IMAGE_EDGE_MAX: u32 = 384;
/// Hard head-read cap for text snippets. A 60 GB log file costs 16 KiB.
pub const TILE_TEXT_HEAD: u64 = 16 * 1024;
/// Most snippet lines handed to the UI; the draw side cuts further by height.
pub const TILE_TEXT_LINES: usize = 40;
pub const TILE_CACHE_ENTRIES: usize = 128;
pub const TILE_CACHE_BYTES: usize = 48 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    Image,
    Text,
    SvgText,
    /// Folder summaries are computed from the in-memory tree by the app, never
    /// from disk. The service returns `None` for this kind.
    Folder,
    /// Archives, Office packages and disk images: entry listings.
    List,
    /// Read-only SQLite schema/table listing.
    Db,
    /// Audio/video tags, cover art, or a rich shell thumbnail.
    Media,
    /// Windows executable/DLL version resource.
    Executable,
    /// PDF, font, disk image, compressed container or unknown binary facts.
    Meta,
    /// Bounded hex preview when nothing better is available.
    Hex,
}

/// Small label drawn above a tile's factual lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactKind {
    List,
    Db,
    Media,
    Executable,
    Meta,
    Hex,
}

impl FactKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::List => "LIST",
            Self::Db => "SQLITE",
            Self::Media => "MEDIA",
            Self::Executable => "BINARY",
            Self::Meta => "META",
            Self::Hex => "HEX",
        }
    }
}

/// Routes a file name to an on-tile preview family. Every file gets a family;
/// unknown files fall back to metadata/hex facts rather than a bare icon.
pub fn tile_kind_for_name(name: &str) -> Option<TileKind> {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() && ext.len() <= 16 => (name, ext),
        _ => (name, ""),
    };
    if stem.is_empty() {
        return Some(TileKind::Meta);
    }
    let ext = ext.to_ascii_lowercase();
    match ext.as_str() {
        // Only codecs the bundled decoder actually supports are decoded
        // in-process; unsupported images fall back to rich/metadata.
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "ico" | "heic"
        | "avif" | "jxl" => Some(TileKind::Image),
        // SVG is displayed as sanitized source text, never rendered.
        "svg" => Some(TileKind::SvgText),
        "txt" | "md" | "rtf" | "log" | "out" | "rs" | "tsx" | "jsx" | "mjs" | "cjs" | "py"
        | "java" | "c" | "cpp" | "cc" | "cxx" | "h" | "hpp" | "hxx" | "cs" | "go" | "sh"
        | "bash" | "zsh" | "ps1" | "bat" | "cmd" | "sql" | "vue" | "svelte" | "rb" | "php"
        | "pl" | "pm" | "swift" | "kt" | "kts" | "scala" | "lua" | "r" | "jl" | "dart" | "ex"
        | "exs" | "erl" | "hs" | "ml" | "asm" | "s" | "json" | "jsonl" | "yaml" | "yml"
        | "toml" | "xml" | "csv" | "tsv" | "ini" | "cfg" | "conf" | "env" | "properties"
        | "reg" | "plist" | "pem" | "crt" | "cer" | "asc" | "key" | "pub" | "eml" | "mbox"
        | "srt" | "ass" | "ssa" | "vtt" | "html" | "htm" | "css" | "scss" | "mts" | "cts"
        | "ts" => Some(TileKind::Text),
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "m2ts" | "mp3" | "wav" | "flac"
        | "m4a" | "aac" | "ogg" | "oga" | "opus" | "wma" => Some(TileKind::Media),
        "zip" | "epub" | "tar" | "iso" | "img" | "pptx" | "odp" | "xlsx" | "ods" | "docx"
        | "odt" | "jar" | "apk" | "cbz" | "xpi" | "vsix" | "nupkg" => Some(TileKind::List),
        "db" | "sqlite" | "sqlite3" | "db3" | "s3db" => Some(TileKind::Db),
        "exe" | "dll" => Some(TileKind::Executable),
        "bin" | "dat" | "dmp" | "pak" | "mdb" | "accdb" | "tmp" | "temp" | "part"
        | "crdownload" | "idx" | "sub" | "fbx" | "blend" | "dwg" | "glb" => Some(TileKind::Hex),
        _ => Some(TileKind::Meta),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileArtKey {
    pub path: PathBuf,
    pub len: u64,
    pub mtime_nanos: u128,
    pub kind: TileKind,
    pub edge: u32,
    /// Rich and in-process results are cached separately so toggling the
    /// setting never serves a stale family.
    pub rich: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TileArt {
    /// Tight RGBA8 thumbnail, aspect-fit by the draw side.
    Image {
        width: u32,
        height: u32,
        rgba: Arc<[u8]>,
    },
    Text {
        lines: Arc<[String]>,
    },
    /// Factual rows (listing, schema, tags, version resource, metadata, hex)
    /// drawn above the tile name by the canvas.
    Facts {
        kind: FactKind,
        lines: Arc<[String]>,
    },
    Folder {
        files: u64,
        items: u32,
        categories: CategoryTotals,
    },
    /// Decode failed or input unsupported: the UI falls back to the icon and
    /// never requests this tile again.
    Unavailable,
}

impl TileArt {
    pub fn weight(&self) -> usize {
        match self {
            Self::Image { rgba, .. } => rgba.len().saturating_add(64),
            Self::Text { lines } | Self::Facts { lines, .. } => {
                lines.iter().fold(48usize, |n, line| {
                    n.saturating_add(line.len()).saturating_add(24)
                })
            }
            Self::Folder { .. } => 96,
            Self::Unavailable => 32,
        }
    }
}

fn art_weight(art: &Arc<TileArt>) -> usize {
    art.weight()
}

struct TileCache {
    inner: Lru<TileArtKey, Arc<TileArt>>,
}

impl TileCache {
    fn new() -> Self {
        Self {
            inner: Lru::new(TILE_CACHE_ENTRIES, TILE_CACHE_BYTES, art_weight),
        }
    }
}

/// Bounded tile-art service, cheap to clone. Disk work is limited to the same
/// four-permit gate pattern as the inspector preview service, here set to
/// three concurrent jobs so tile loading never starves explicit previews.
#[derive(Clone)]
pub struct TileService {
    inner: Arc<TileServiceInner>,
}

struct TileServiceInner {
    cache: Mutex<TileCache>,
    gate: Gate,
}

impl Default for TileService {
    fn default() -> Self {
        Self::new()
    }
}

impl TileService {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TileServiceInner {
                cache: Mutex::new(TileCache::new()),
                gate: Gate::new(3),
            }),
        }
    }

    /// Returns cached or freshly built tile art. Never panics: malformed or
    /// over-limit files degrade to `None` (icon fallback). Folder kinds are
    /// never read here.
    pub fn art(&self, path: &Path, kind: TileKind, edge: u32, rich: bool) -> Option<Arc<TileArt>> {
        if matches!(kind, TileKind::Folder) || path.as_os_str().is_empty() {
            return None;
        }
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return None;
        }
        let key = TileArtKey {
            path: path.to_path_buf(),
            len: metadata.len(),
            mtime_nanos: mtime_nanos(&metadata),
            kind,
            edge,
            rich,
        };

        if let Some(hit) = self.inner.cache.lock().inner.get(&key) {
            return Some(hit);
        }

        let _permit = self.inner.gate.acquire();
        if let Some(hit) = self.inner.cache.lock().inner.get(&key) {
            return Some(hit);
        }

        let built = panic::catch_unwind(AssertUnwindSafe(|| build(path, &metadata, &key)))
            .unwrap_or(None)
            .map(Arc::new);

        if let Some(art) = &built {
            self.inner.cache.lock().inner.insert(key, Arc::clone(art));
        }
        built
    }

    pub fn cache_len(&self) -> usize {
        self.inner.cache.lock().inner.len()
    }
}

fn build(path: &Path, metadata: &std::fs::Metadata, key: &TileArtKey) -> Option<TileArt> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match key.kind {
        TileKind::Folder => None,
        TileKind::Image => build_image(path, key, &ext),
        TileKind::Text | TileKind::SvgText => build_text(path, key),
        TileKind::List => build_list(path, key, &ext),
        TileKind::Db => build_db(path, key),
        TileKind::Media => build_media(path, key, &ext),
        TileKind::Executable => build_executable(path, metadata, key),
        TileKind::Meta => build_meta(path, metadata, key, &ext),
        TileKind::Hex => build_hex(path, key),
    }
}

fn image_art(image: super::ImagePreview) -> TileArt {
    TileArt::Image {
        width: image.width,
        height: image.height,
        rgba: image.rgba,
    }
}

fn build_image(path: &Path, key: &TileArtKey, ext: &str) -> Option<TileArt> {
    if let Some(image) = images::build_tile(path, key.len, key.edge) {
        return Some(image_art(image));
    }
    if key.rich
        && let Some(thumb) = rich_thumbnail(path, key, 512)
    {
        return Some(thumb);
    }
    // Unsupported codecs still get honest signature/size rows.
    if matches!(ext, "heic" | "avif" | "jxl") {
        let lines = vec![format!("{ext} · no in-process decoder"), fmt_bytes(key.len)];
        return Some(TileArt::Facts {
            kind: FactKind::Meta,
            lines: lines.into(),
        });
    }
    None
}

fn build_text(path: &Path, key: &TileArtKey) -> Option<TileArt> {
    if key.len == 0 {
        return Some(TileArt::Facts {
            kind: FactKind::Meta,
            lines: vec!["empty file · 0 B".to_string()].into(),
        });
    }
    let raw = read_head(path, key.len.min(TILE_TEXT_HEAD)).ok()?;
    let preview = text::from_head(&raw, key.len, key.kind == TileKind::SvgText)?;
    let lines: Arc<[String]> = preview
        .lines
        .into_iter()
        .take(TILE_TEXT_LINES)
        .collect::<Vec<_>>()
        .into();
    if lines.is_empty() {
        Some(TileArt::Facts {
            kind: FactKind::Meta,
            lines: vec![format!("text · {}", fmt_bytes(key.len))].into(),
        })
    } else {
        Some(TileArt::Text { lines })
    }
}

fn list_preview(path: &Path, ext: &str, len: u64) -> Option<super::ListPreview> {
    match ext {
        "tar" => list::tar(path, len).ok(),
        "iso" | "img" => list::iso(path, len).ok(),
        "xlsx" | "ods" => list::sheet(path, ext).ok(),
        "docx" | "odt" => list::office(path, ext).ok(),
        _ => list::zip(path).ok(),
    }
}

fn build_list(path: &Path, key: &TileArtKey, ext: &str) -> Option<TileArt> {
    if key.rich
        && matches!(
            ext,
            "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "epub"
        )
        && let Some(thumb) = rich_thumbnail(path, key, 384)
    {
        return Some(thumb);
    }
    let mut preview = list_preview(path, ext, key.len)?;
    let mut lines: Vec<String> = Vec::new();
    let total = preview.total.unwrap_or(preview.entries.len() as u64);
    lines.push(format!(
        "{} · {} entries{}",
        preview.kind.to_ascii_uppercase(),
        total,
        if preview.truncated { " · capped" } else { "" }
    ));
    for entry in preview.entries.drain(..).take(10) {
        let name = scrub_inline(&entry.name);
        let size = entry
            .size
            .map(fmt_bytes)
            .unwrap_or_else(|| "dir".to_string());
        lines.push(format!("{name} · {size}"));
    }
    Some(TileArt::Facts {
        kind: FactKind::List,
        lines: lines.into(),
    })
}

fn build_db(path: &Path, key: &TileArtKey) -> Option<TileArt> {
    let preview = db::build(path).ok()?;
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "sqlite · {} tables{}",
        preview.tables.len(),
        if preview.truncated { " · capped" } else { "" }
    ));
    for table in preview.tables.iter().take(9) {
        let name = scrub_inline(&table.name);
        lines.push(format!("{name} · {} cols", table.cols));
    }
    if lines.len() == 1 {
        lines.push(fmt_bytes(key.len));
    }
    Some(TileArt::Facts {
        kind: FactKind::Db,
        lines: lines.into(),
    })
}

fn build_media(path: &Path, key: &TileArtKey, ext: &str) -> Option<TileArt> {
    if key.rich
        && media::is_video(ext)
        && let Some(thumb) = rich_thumbnail(path, key, 384)
    {
        return Some(thumb);
    }
    let preview = media::build(path, ext, key.len);
    if let Some(thumbnail) = preview.thumbnail {
        return Some(image_art(thumbnail));
    }
    if key.rich
        && let Some(thumb) = rich_thumbnail(path, key, 384)
    {
        return Some(thumb);
    }
    let mut lines = Vec::with_capacity(preview.pairs.len() + 1);
    lines.push(format!(
        "{} · {}",
        preview.kind.to_ascii_uppercase(),
        preview.mime
    ));
    for pair in preview.pairs.iter().take(9) {
        lines.push(format!("{} · {}", pair.key, scrub_inline(&pair.value)));
    }
    Some(TileArt::Facts {
        kind: FactKind::Media,
        lines: lines.into(),
    })
}

fn build_executable(
    path: &Path,
    metadata: &std::fs::Metadata,
    key: &TileArtKey,
) -> Option<TileArt> {
    let mut lines = Vec::new();
    #[cfg(windows)]
    lines.extend(super::winmeta::version_lines(path));
    if let super::Preview::Meta(preview) = meta::build(path, metadata, "exe") {
        for pair in preview.pairs.iter().take(6) {
            let key = pair.key.as_str();
            if matches!(
                key,
                "type" | "signature" | "modified" | "created" | "accessed"
            ) {
                continue;
            }
            lines.push(format!("{key} · {}", scrub_inline(&pair.value)));
        }
    }
    if lines.is_empty() {
        return build_hex(path, key);
    }
    lines.push(format!("size · {}", fmt_bytes(key.len)));
    lines.truncate(9);
    Some(TileArt::Facts {
        kind: FactKind::Executable,
        lines: lines.into(),
    })
}

fn build_meta(
    path: &Path,
    metadata: &std::fs::Metadata,
    key: &TileArtKey,
    ext: &str,
) -> Option<TileArt> {
    if key.rich
        && matches!(
            ext,
            "pdf"
                | "ttf"
                | "otf"
                | "ttc"
                | "woff"
                | "woff2"
                | "fon"
                | "mobi"
                | "epub"
                | "ai"
                | "eps"
        )
        && let Some(thumb) = rich_thumbnail(path, key, 384)
    {
        return Some(thumb);
    }
    let super::Preview::Meta(preview) = meta::build(path, metadata, ext) else {
        return build_hex(path, key);
    };
    let mut lines: Vec<String> = Vec::new();
    for pair in preview.pairs.iter() {
        let name = pair.key.as_str();
        if matches!(
            name,
            "type" | "signature" | "modified" | "created" | "accessed" | "attributes"
        ) {
            continue;
        }
        lines.push(format!("{name} · {}", scrub_inline(&pair.value)));
        if lines.len() >= 8 {
            break;
        }
    }
    if lines.is_empty() {
        lines.push(format!("file · {}", fmt_bytes(key.len)));
    }
    Some(TileArt::Facts {
        kind: FactKind::Meta,
        lines: lines.into(),
    })
}

fn build_hex(path: &Path, key: &TileArtKey) -> Option<TileArt> {
    let preview = hex::build(path, key.len).ok()?;
    let mut lines = Vec::with_capacity(9);
    for row in preview.rows.iter().take(8) {
        lines.push(format!("{:08x}  {}  |{}|", row.offset, row.hex, row.ascii));
    }
    if lines.is_empty() {
        lines.push(format!("hex · {}", fmt_bytes(key.len)));
    }
    Some(TileArt::Facts {
        kind: FactKind::Hex,
        lines: lines.into(),
    })
}

#[cfg(windows)]
fn rich_thumbnail(path: &Path, key: &TileArtKey, edge: u32) -> Option<TileArt> {
    let image = super::winthumb::thumbnail(path, edge.min(TILE_IMAGE_EDGE_MAX), key.len)?;
    Some(image_art(image))
}

#[cfg(not(windows))]
fn rich_thumbnail(_path: &Path, _key: &TileArtKey, _edge: u32) -> Option<TileArt> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::{Seek, SeekFrom, Write};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "space-tile-art-tests-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create fixture dir");
        dir
    }

    #[test]
    fn image_over_the_byte_cap_is_rejected_without_reading_it() {
        let dir = fixture_dir("cap");
        let path = dir.join("huge.png");
        {
            // Sparse 64 MiB file: larger than the 32 MiB decoder cap.
            let mut file = File::create(&path).expect("create sparse fixture");
            file.write_all(b"\x89PNG\r\n\x1a\n").expect("write header");
            file.set_len(64 * 1024 * 1024).expect("extend sparse");
        }
        let service = TileService::new();
        assert_eq!(service.art(&path, TileKind::Image, 256, false), None);
        assert_eq!(service.cache_len(), 0, "rejected art must not be cached");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_snippets_read_only_a_bounded_head() {
        let dir = fixture_dir("head");
        let path = dir.join("huge.log");
        {
            let mut file = File::create(&path).expect("create fixture");
            writeln!(file, "HEAD-LINE").expect("write head");
            let filler = vec![b'a'; 16 * 1024];
            file.write_all(&filler).expect("write filler");
            file.seek(SeekFrom::Start(128 * 1024)).expect("seek");
            writeln!(file, "TAIL-MARKER-BEYOND-THE-CAP").expect("write tail");
        }
        let service = TileService::new();
        let art = service
            .art(&path, TileKind::Text, 256, false)
            .expect("text art");
        let TileArt::Text { lines } = art.as_ref() else {
            panic!("expected text art");
        };
        assert_eq!(lines.first().map(String::as_str), Some("HEAD-LINE"));
        assert!(
            !lines.iter().any(|line| line.contains("TAIL-MARKER")),
            "content beyond the bounded head must never be read"
        );
        assert!(lines.len() <= TILE_TEXT_LINES);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_utf8_bom_tile_starts_at_the_first_real_character() {
        let dir = fixture_dir("bom");
        let path = dir.join("QA.md");
        fs::write(&path, b"\xEF\xBB\xBF# QA\n\nAcceptance.").expect("write bom fixture");
        let service = TileService::new();
        let art = service
            .art(&path, TileKind::Text, 256, false)
            .expect("text art");
        let TileArt::Text { lines } = art.as_ref() else {
            panic!("expected text art");
        };
        assert_eq!(lines.first().map(String::as_str), Some("# QA"));
        assert!(
            lines.iter().all(|line| !line.contains('\u{FEFF}')),
            "no BOM glyph may leak into the first tile row"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn decode_failure_returns_none_for_icon_fallback() {
        let dir = fixture_dir("corrupt");
        let path = dir.join("broken.png");
        fs::write(&path, b"not a png at all, just text bytes").expect("write corrupt fixture");
        let service = TileService::new();
        assert_eq!(service.art(&path, TileKind::Image, 256, false), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_real_png_decodes_to_a_bounded_thumbnail() {
        let dir = fixture_dir("png");
        let path = dir.join("thumb.png");
        // 2x2 RGBA PNG built by the project's own encoder.
        let mut bytes = Vec::new();
        {
            let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
            image
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .expect("encode png");
        }
        fs::write(&path, &bytes).expect("write png fixture");
        let service = TileService::new();
        let art = service
            .art(&path, TileKind::Image, 64, false)
            .expect("thumbnail art");
        let TileArt::Image { width, height, .. } = art.as_ref() else {
            panic!("expected image art");
        };
        assert_eq!((*width, *height), (2, 2));
        // Second call is a cache hit, not a rebuild.
        assert!(service.art(&path, TileKind::Image, 64, false).is_some());
        assert_eq!(service.cache_len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_file_name_routes_to_a_preview_family() {
        assert_eq!(tile_kind_for_name("disk.vdi"), Some(TileKind::Meta));
        assert_eq!(tile_kind_for_name("app.exe"), Some(TileKind::Executable));
        assert_eq!(tile_kind_for_name("driver.dll"), Some(TileKind::Executable));
        assert_eq!(tile_kind_for_name("archive.zip"), Some(TileKind::List));
        assert_eq!(tile_kind_for_name("book.pdf"), Some(TileKind::Meta));
        assert_eq!(tile_kind_for_name("movie.mp4"), Some(TileKind::Media));
        assert_eq!(tile_kind_for_name("song.flac"), Some(TileKind::Media));
        assert_eq!(tile_kind_for_name("main.db"), Some(TileKind::Db));
        assert_eq!(tile_kind_for_name("photo.jpg"), Some(TileKind::Image));
        assert_eq!(tile_kind_for_name("notes.md"), Some(TileKind::Text));
        assert_eq!(tile_kind_for_name("vector.svg"), Some(TileKind::SvgText));
        assert_eq!(tile_kind_for_name("payload.bin"), Some(TileKind::Hex));
        assert_eq!(tile_kind_for_name("LICENSE"), Some(TileKind::Meta));
        assert_eq!(tile_kind_for_name("unknown.xyzzy"), Some(TileKind::Meta));
    }

    #[test]
    fn folder_kinds_never_read_from_disk() {
        let dir = fixture_dir("folder");
        let path = dir.join("disk.vdi");
        fs::write(&path, b"should never be opened").expect("write fixture");
        let service = TileService::new();
        assert_eq!(service.art(&path, TileKind::Folder, 256, false), None);
        assert_eq!(service.art(&path, TileKind::Folder, 256, true), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_like_unknown_files_get_metadata_instead_of_an_icon() {
        let dir = fixture_dir("meta");
        let path = dir.join("disk.vdi");
        fs::write(&path, b"Conectix virtual disk fixture").expect("write fixture");
        let service = TileService::new();
        let art = service
            .art(&path, TileKind::Meta, 256, false)
            .expect("metadata facts");
        let TileArt::Facts { kind, lines } = art.as_ref() else {
            panic!("expected facts");
        };
        assert_eq!(*kind, FactKind::Meta);
        assert!(!lines.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archives_list_their_first_entries() {
        let dir = fixture_dir("zip");
        let path = dir.join("bundle.zip");
        {
            let file = File::create(&path).expect("create zip");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("alpha.txt", options).expect("entry");
            writer.write_all(b"alpha").expect("write entry");
            writer.start_file("beta/notes.md", options).expect("entry");
            writer.write_all(b"beta").expect("write entry");
            writer.finish().expect("finish zip");
        }
        let service = TileService::new();
        let art = service
            .art(&path, TileKind::List, 256, false)
            .expect("listing facts");
        let TileArt::Facts { kind, lines } = art.as_ref() else {
            panic!("expected facts");
        };
        assert_eq!(*kind, FactKind::List);
        assert!(lines.iter().any(|line| line.starts_with("ZIP ·")));
        assert!(lines.iter().any(|line| line.starts_with("alpha.txt")));
        assert!(lines.iter().any(|line| line.starts_with("beta/notes.md")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sqlite_tiles_name_their_tables() {
        let dir = fixture_dir("sqlite");
        let path = dir.join("app.db");
        {
            let conn = rusqlite::Connection::open(&path).expect("create db");
            conn.execute_batch(
                "CREATE TABLE users (id INTEGER, name TEXT);\
                 CREATE TABLE events (id INTEGER, at TEXT);",
            )
            .expect("schema");
        }
        let service = TileService::new();
        let art = service
            .art(&path, TileKind::Db, 256, false)
            .expect("db facts");
        let TileArt::Facts { kind, lines } = art.as_ref() else {
            panic!("expected facts");
        };
        assert_eq!(*kind, FactKind::Db);
        assert!(lines.iter().any(|line| line.starts_with("users ·")));
        assert!(lines.iter().any(|line| line.starts_with("events ·")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rich_flag_is_part_of_the_cache_key() {
        let dir = fixture_dir("rich");
        let path = dir.join("notes.txt");
        fs::write(&path, b"hello rich previews").expect("write fixture");
        let service = TileService::new();
        assert!(service.art(&path, TileKind::Text, 256, false).is_some());
        assert!(service.art(&path, TileKind::Text, 256, true).is_some());
        assert_eq!(
            service.cache_len(),
            2,
            "the setting must not reuse a result from the other mode"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tile_cache_evicts_by_count_and_bytes() {
        let mut cache = TileCache::new();
        let big = Arc::new(TileArt::Image {
            width: 64,
            height: 64,
            rgba: vec![0u8; 1024].into(),
        });
        let key = |i: u64| TileArtKey {
            path: PathBuf::from(format!("f{i}.png")),
            len: i,
            mtime_nanos: i as u128,
            kind: TileKind::Image,
            edge: 64,
            rich: false,
        };
        for i in 0..(TILE_CACHE_ENTRIES as u64 + 20) {
            cache.inner.insert(key(i), Arc::clone(&big));
            assert!(cache.inner.len() <= TILE_CACHE_ENTRIES);
            assert!(cache.inner.bytes() <= TILE_CACHE_BYTES);
        }
        assert!(
            cache.inner.get(&key(0)).is_none(),
            "count cap evicts oldest"
        );
        assert!(
            cache
                .inner
                .get(&key(TILE_CACHE_ENTRIES as u64 + 19))
                .is_some()
        );

        // Byte eviction: entries near the 48 MiB budget.
        let huge = Arc::new(TileArt::Image {
            width: 384,
            height: 384,
            rgba: vec![1u8; 8 * 1024 * 1024].into(),
        });
        let mut bytes_cache = TileCache::new();
        for i in 0..8 {
            bytes_cache.inner.insert(key(i), Arc::clone(&huge));
            assert!(bytes_cache.inner.bytes() <= TILE_CACHE_BYTES);
        }
        assert!(bytes_cache.inner.len() < 8, "byte cap must evict");
        assert!(bytes_cache.inner.get(&key(0)).is_none());
    }
}
