//! Classification of files into the category ids used by the UI.
//!
//! Two independent rules decide a file's category:
//! 1. The *directory* the file lives in may be special (a `node_modules`
//!    folder, a `.cache`, ...). Any path component matching a known
//!    "cache-ish" directory forces `CAT_CACHES`.
//! 2. Otherwise the file's *extension* decides the category.
//!
//! Rule 1 takes priority over rule 2, except on Windows where a path under
//! `C:\Windows` is system software and always wins.
//!
//! Both rules run with ZERO heap allocations. Directory flags are decided
//! once per directory by an in-place scan of the path bytes/UTF-16 (the old
//! version built a lowercased `String` of the whole path per directory). The
//! extension lookup parses the extension straight out of the file name — as
//! `str` bytes, raw bytes, or UTF-16 straight from `WIN32_FIND_DATAW` — into
//! a small stack buffer and dispatches through one static `match` (the
//! compiler lowers it to a decision tree keyed on the extension's length and
//! leading bytes, i.e. the (length, first char) bucket dispatch: ~1-3
//! compares per file, no `to_lowercase()` allocation, no linear scan over
//! five extension tables).

use std::path::Path;

use crate::model::{CAT_APPS, CAT_ARCHIVES, CAT_CACHES, CAT_CODE, CAT_DOCUMENTS, CAT_MEDIA, CAT_SYSTEM, CAT_OTHER};

/// Directory component names that mark a "cache / dependency" location.
/// Compared case-insensitively against every component of the path.
const CACHE_DIRS: &[&str] = &[
    "node_modules",
    "__pycache__",
    ".cache",
    "cache",
    "tmp",
    "temp",
    "target",
    ".gradle",
    ".venv",
    "venv",
    ".npm",
    ".cargo",
    ".rustup",
];

/// Longest extension that can still be matched. The longest known entry is
/// `appimage` (8 bytes); any extension longer than [`MAX_EXT`] bytes is
/// `CAT_OTHER` regardless of content, exactly like the old table miss, so
/// bounding the stack buffer here changes nothing observable.
const MAX_EXT: usize = 16;

/// Directory-level classification flags, precomputed once per directory.
///
/// `Copy` on purpose: passing it into [`file_category`] for every file of the
/// directory is free.
#[derive(Clone, Copy)]
pub struct DirFlags {
    /// The directory sits under `C:\Windows` (Windows only).
    under_windows: bool,
    /// Some component of the directory path marks a cache/dependency folder.
    in_cache: bool,
}

/// True when running on Windows.
#[cfg(target_os = "windows")]
fn is_windows() -> bool {
    true
}

/// True when running on any platform other than Windows.
#[cfg(not(target_os = "windows"))]
fn is_windows() -> bool {
    false
}

/// Case-insensitive ASCII prefix test on raw path bytes. Equivalent to the
/// old `to_lowercase().starts_with("c:\\windows")`: the needle is pure ASCII,
/// and no Unicode lowercase mapping can turn a non-ASCII byte into an ASCII
/// one inside an equality with an ASCII needle.
#[cfg_attr(windows, allow(dead_code))]
fn ascii_ci_starts_with(hay: &[u8], needle: &[u8]) -> bool {
    hay.len() >= needle.len()
        && hay
            .iter()
            .zip(needle)
            .all(|(h, n)| h.to_ascii_lowercase() == *n)
}

/// ASCII-lowers a UTF-16 code unit; non-ASCII units stay out of ASCII range
/// so they can never equal a (pure-ASCII) table byte.
#[inline]
fn lower_u16(c: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&c) {
        c + (b'a' as u16 - b'A' as u16)
    } else {
        c
    }
}

/// Case-insensitive ASCII equality between a UTF-16 component and a byte
/// needle. Non-ASCII code units can never equal an ASCII table byte, so they
/// simply never match.
#[inline]
fn u16_ci_eq(comp: &[u16], needle: &[u8]) -> bool {
    comp.len() == needle.len()
        && comp
            .iter()
            .zip(needle)
            .all(|(&c, n)| lower_u16(c) == *n as u16)
}

/// Computes the [`DirFlags`] of `dir_path` with no allocations: the path is
/// scanned in place, splitting on both separators (same rule as the old
/// lowercased `split(['\\', '/'])` — a Unix filename containing a literal
/// backslash still splits, keeping the old component semantics).
///
/// Only the jwalk fallback (non-Windows) reaches this from a `Path`; kept
/// available on every platform as the stable `Path`-based API.
#[cfg_attr(windows, allow(dead_code))]
pub fn dir_flags(dir_path: &Path) -> DirFlags {
    let bytes = dir_path.as_os_str().as_encoded_bytes();
    // Priority 2: on Windows, everything under C:\Windows belongs to the OS.
    // (The drive letter is the hardcoded `c:` of the original rule.)
    let under_windows = is_windows() && ascii_ci_starts_with(bytes, b"c:\\windows");
    // Priority 1: a special directory anywhere in the path. Windows paths
    // also carry a drive prefix such as `c:` which never matches the names
    // below. The component scan is skipped under C:\Windows: that rule
    // already wins, and C:\Windows is exactly where the skipped work adds up.
    let in_cache = !under_windows
        && bytes
            .split(|&b| b == b'\\' || b == b'/')
            .any(|comp| !comp.is_empty() && CACHE_DIRS.iter().any(|d| comp.eq_ignore_ascii_case(d.as_bytes())));
    DirFlags {
        under_windows,
        in_cache,
    }
}

/// Zero-alloc [`dir_flags`] for a directory path already available as UTF-16
/// (the Windows fast path carries the path in that form for
/// `FindFirstFileExW` and never needs a `PathBuf`).
pub fn dir_flags_utf16(path: &[u16]) -> DirFlags {
    let under_windows = is_windows() && {
        let needle = b"c:\\windows";
        path.len() >= needle.len()
            && path
                .iter()
                .zip(needle)
                .all(|(&c, n)| lower_u16(c) == *n as u16)
    };
    let in_cache = !under_windows
        && path
            .split(|&c| c == '\\' as u16 || c == '/' as u16)
            .any(|comp| !comp.is_empty() && CACHE_DIRS.iter().any(|d| u16_ci_eq(comp, d.as_bytes())));
    DirFlags {
        under_windows,
        in_cache,
    }
}

/// Parses the extension from `name`'s bytes — last dot after the first
/// character, mirroring `Path::extension` semantics (`file.tar.gz` → `gz`,
/// `.gitignore` → none) — lowercasing ASCII in place into `buf`.
///
/// Returns `None` for "no extension" and for extensions containing non-ASCII
/// bytes: those can never equal an ASCII table entry (no Unicode lowercase
/// mapping produces an ASCII byte inside such an equality), which is exactly
/// what the old `to_lowercase` + table-scan did for them.
#[cfg_attr(windows, allow(dead_code))]
#[inline]
fn extension_bytes<'a>(name: &[u8], buf: &'a mut [u8; MAX_EXT]) -> Option<&'a [u8]> {
    let mut dot = None;
    let mut index = name.len();
    while index > 0 {
        index -= 1;
        if name[index] == b'.' {
            dot = Some(index);
            break;
        }
    }
    let dot = dot?;
    if dot == 0 {
        // Leading-dot names without a second dot have no extension.
        return None;
    }
    let raw = &name[dot + 1..];
    if raw.len() > MAX_EXT {
        return None;
    }
    for (slot, &byte) in buf[..raw.len()].iter_mut().zip(raw) {
        *slot = byte.to_ascii_lowercase();
    }
    Some(&buf[..raw.len()])
}

/// Same as [`extension_bytes`] but reading a UTF-16 name (the Windows fast
/// path hands over `find_data.cFileName` without ever materializing a
/// `String`).
#[inline]
fn extension_utf16<'a>(name: &[u16], buf: &'a mut [u8; MAX_EXT]) -> Option<&'a [u8]> {
    let mut dot = None;
    let mut index = name.len();
    while index > 0 {
        index -= 1;
        if name[index] == b'.' as u16 {
            dot = Some(index);
            break;
        }
    }
    let dot = dot?;
    if dot == 0 {
        return None;
    }
    let raw = &name[dot + 1..];
    if raw.len() > MAX_EXT {
        return None;
    }
    let mut filled = 0usize;
    for &unit in raw {
        // Trailing dot ("foo.") and non-ASCII units both fold into the same
        // outcomes the old code produced (empty ext / never-matching ext).
        if unit >= 128 {
            return None;
        }
        buf[filled] = (unit as u8).to_ascii_lowercase();
        filled += 1;
    }
    Some(&buf[..filled])
}

/// The extension dispatch table. One static `match` on the lowercased bytes:
/// rustc lowers byte-string literal patterns on a slice to a decision tree
/// split first on length, then on leading bytes — the (length, first char)
/// bucket lookup — followed by one fixed-size compare per bucket. Extensions
/// here are ordered roughly by real-world frequency so the common cases sit
/// early in each bucket.
fn classify_ext(ext: &[u8]) -> u8 {
    match ext {
        // media — images
        b"jpg" | b"jpeg" | b"png" | b"gif" | b"webp" | b"bmp" | b"svg" | b"heic" | b"raw"
        | b"tiff" | b"ico"
        // media — video
        | b"mp4" | b"mov" | b"avi" | b"mkv" | b"webm" | b"m4v" | b"wmv" | b"flv"
        // media — audio
        | b"mp3" | b"wav" | b"flac" | b"aac" | b"ogg" | b"m4a" | b"wma" => CAT_MEDIA,

        // documents
        b"pdf" | b"doc" | b"docx" | b"xls" | b"xlsx" | b"ppt" | b"pptx" | b"txt" | b"md"
        | b"rtf" | b"odt" | b"ods" | b"csv" | b"epub" | b"mobi" => CAT_DOCUMENTS,

        // code — compiled / scripting languages
        b"rs" | b"ts" | b"tsx" | b"js" | b"jsx" | b"mjs" | b"cjs" | b"py" | b"java" | b"kt"
        | b"go" | b"c" | b"cpp" | b"h" | b"hpp" | b"cs" | b"rb" | b"php" | b"swift" | b"m"
        | b"mm" | b"sh" | b"ps1" | b"bat" | b"sql"
        // code — web / data
        | b"html" | b"css" | b"scss" | b"sass" | b"less" | b"vue" | b"svelte" | b"json"
        | b"yaml" | b"yml" | b"toml" | b"xml" | b"lock" | b"ipynb" | b"lua" | b"dart"
        | b"zig" => CAT_CODE,

        // apps
        b"exe" | b"msi" | b"msix" | b"appx" | b"dll" | b"sys" | b"drv" | b"so" | b"dylib"
        | b"app" | b"deb" | b"rpm" | b"appimage" | b"dmg" | b"apk" | b"jar" => CAT_APPS,

        // archives
        b"zip" | b"rar" | b"7z" | b"tar" | b"gz" | b"tgz" | b"bz2" | b"xz" | b"zst" | b"iso"
        | b"img" | b"vhd" | b"vhdx" => CAT_ARCHIVES,

        _ => CAT_OTHER,
    }
}

/// Category id for a file called `name` that lives inside a directory whose
/// flags were precomputed with [`dir_flags`].
///
/// See the module docs for the two priority rules. Zero allocations.
#[cfg_attr(windows, allow(dead_code))]
pub fn file_category(name: &str, flags: DirFlags) -> u8 {
    file_category_bytes(name.as_bytes(), flags)
}

/// Raw-bytes variant of [`file_category`] for names that are not (yet)
/// `String`s (e.g. Unix `OsStr` bytes). Zero allocations.
#[cfg_attr(windows, allow(dead_code))]
pub fn file_category_bytes(name: &[u8], flags: DirFlags) -> u8 {
    // Priority 2: on Windows, everything under C:\Windows belongs to the OS.
    if flags.under_windows {
        return CAT_SYSTEM;
    }

    // Priority 1: a special directory anywhere in the parent path.
    if flags.in_cache {
        return CAT_CACHES;
    }

    // Priority 3: fall back to the file extension.
    let mut buf = [0u8; MAX_EXT];
    match extension_bytes(name, &mut buf) {
        Some(ext) => classify_ext(ext),
        None => CAT_OTHER,
    }
}

/// UTF-16 variant of [`file_category`]: parses the extension straight from
/// the `WIN32_FIND_DATAW` name buffer, so the hot loop stays allocation-free.
pub fn file_category_utf16(name: &[u16], flags: DirFlags) -> u8 {
    if flags.under_windows {
        return CAT_SYSTEM;
    }
    if flags.in_cache {
        return CAT_CACHES;
    }
    let mut buf = [0u8; MAX_EXT];
    match extension_utf16(name, &mut buf) {
        Some(ext) => classify_ext(ext),
        None => CAT_OTHER,
    }
}
