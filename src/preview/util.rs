//! Shared bounded I/O, formatting and text-scrubbing helpers.
//!
//! Everything here is read-only: files are opened with `File::open`, reads are
//! capped by the caller, and no API that can create/write/rename a path is
//! used. Errors are turned into short, lowercase reasons suitable for a plate.

use std::fs::{File, Metadata};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// TEXT head read (also used for SVG, docx/odt XML and sheet manifests).
pub const TEXT_CAP: u64 = 64 * 1024;
/// HEX head read.
pub const HEX_CAP: u64 = 4 * 1024;
/// META parser head read.
pub const META_HEAD: u64 = 64 * 1024;
/// Bounded PDF page-marker scan.
pub const PDF_SCAN: u64 = 1024 * 1024;
/// Targeted parser reads (PE tails, font name tables, section tables).
pub const TABLE_READ_CAP: u64 = 16 * 1024;
/// Directory entries that receive metadata before sorting.
pub const DIR_HEAD: usize = 1024;
/// Stop calling a folder total "cheap" past this many entries.
pub const DIR_COUNT_MAX: u64 = 50_000;
/// Returned LIST rows (folder, archive, sheet, headings).
pub const LIST_CAP: usize = 200;

/// Short, lowercase reason for a failed file operation.
pub fn io_reason(e: &std::io::Error) -> String {
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
        std::io::ErrorKind::InvalidInput => "invalid path".to_string(),
        _ => format!("unreadable: {}", e.to_string().to_lowercase()),
    }
}

/// Reads at most `n` bytes from the start of a file (stops cleanly at EOF).
pub fn read_head(path: &Path, n: u64) -> Result<Vec<u8>, String> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path).map_err(|e| io_reason(&e))?;
    read_up_to(&mut file, n)
}

/// Reads at most `n` bytes at `off`.
pub fn read_at(path: &Path, off: u64, n: u64) -> Result<Vec<u8>, String> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path).map_err(|e| io_reason(&e))?;
    file.seek(SeekFrom::Start(off)).map_err(|e| io_reason(&e))?;
    read_up_to(&mut file, n)
}

pub fn read_up_to(file: &mut File, n: u64) -> Result<Vec<u8>, String> {
    let n = usize::try_from(n).unwrap_or(usize::MAX);
    let mut buf = vec![0u8; n];
    let mut filled = 0usize;
    while filled < buf.len() {
        match file.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(k) => filled += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_reason(&e)),
        }
    }
    buf.truncate(filled);
    Ok(buf)
}

/// Stable, cheap cache stamp: path + mtime (nanoseconds) + length.
pub fn mtime_nanos(meta: &Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

pub fn fmt_bytes(n: u64) -> String {
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

/// Lowercase, space-separated hex bytes (matches the HEX family's columns).
pub fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(3));
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Howard Hinnant's civil_from_days; no chrono dependency.
pub fn fmt_unix_utc(secs: u64) -> String {
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

pub fn fmt_system_time(t: SystemTime) -> Option<String> {
    let secs = t.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(fmt_unix_utc(secs))
}

/// Strips C0/C1 control characters (tab is kept), bidi overrides and
/// zero-width format characters. The result is safe to display as one line.
pub fn scrub_line(line: &str) -> String {
    line.chars()
        .filter(|c| {
            if *c == '\t' {
                return true;
            }
            if c.is_control() {
                return false;
            }
            !is_invisible_format(*c)
        })
        .collect()
}

/// One-line scrub for table cells and entry names: controls and tabs become
/// spaces, then runs of whitespace are collapsed.
pub fn scrub_inline(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_space = false;
    for c in input.chars() {
        let c = if c == '\t' || c.is_control() || is_invisible_format(c) {
            ' '
        } else {
            c
        };
        if c == ' ' {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

fn is_invisible_format(c: char) -> bool {
    matches!(
        c,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// Truncates by characters and appends an ellipsis when content was cut.
pub fn truncate_chars(input: &str, max: usize) -> (String, bool) {
    if input.chars().count() <= max {
        return (input.to_string(), false);
    }
    let mut out: String = input.chars().take(max).collect();
    out.push('…');
    (out, true)
}

/// A filename-safe display name, bounded so a malformed archive cannot stuff a
/// megabyte of control characters into the UI.
pub fn display_name(input: &str) -> String {
    let scrubbed = scrub_inline(input);
    let (out, _) = truncate_chars(&scrubbed, 240);
    if out.is_empty() {
        "(unnamed)".to_string()
    } else {
        out
    }
}

pub fn ext_lower(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn pair(key: &str, value: impl Into<String>) -> crate::preview::MetaPair {
    crate::preview::MetaPair {
        key: key.to_string(),
        value: value.into(),
    }
}

pub fn u16le(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off + 2)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

pub fn u32le(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn u64le(b: &[u8], off: usize) -> Option<u64> {
    let s = b.get(off..off + 8)?;
    Some(u64::from_le_bytes(s.try_into().ok()?))
}

pub fn u16be(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off + 2)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

pub fn u32be(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn u64be(b: &[u8], off: usize) -> Option<u64> {
    let s = b.get(off..off + 8)?;
    Some(u64::from_be_bytes(s.try_into().ok()?))
}

pub fn at(head: &[u8], off: usize, sig: &[u8]) -> bool {
    head.len() >= off + sig.len() && &head[off..off + sig.len()] == sig
}
