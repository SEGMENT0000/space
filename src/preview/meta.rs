//! META preview: file facts plus bounded parsers for PE, ELF, Mach-O, PDF,
//! fonts, gzip and common signatures. Every value comes from stat data or
//! bytes actually read; unsupported files still get a truthful signature card.

use std::fs::Metadata;
use std::path::Path;

use super::util::{
    META_HEAD, PDF_SCAN, TABLE_READ_CAP, at, fmt_bytes, fmt_system_time, hex_bytes, pair, read_at,
    read_head, u16be, u16le, u32be, u32le, u64be, u64le,
};
use crate::preview::{MetaPair, MetaPreview, Preview};

pub fn build(path: &Path, meta: &Metadata, ext: &str) -> Preview {
    let len = meta.len();
    let (head, note) = match read_head(path, len.min(META_HEAD)) {
        Ok(h) => (h, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    build_from_head(path, meta, ext, &head, note)
}

pub fn build_from_head(
    path: &Path,
    meta: &Metadata,
    ext: &str,
    head: &[u8],
    note: Option<String>,
) -> Preview {
    let len = meta.len();
    let mut parsed: Vec<MetaPair> = Vec::new();
    let mut has_format = false;

    if head.starts_with(b"MZ") {
        if let Some(p) = parse_pe(path, head) {
            parsed = p;
            has_format = declared_format(&parsed);
        }
    } else if head.starts_with(b"\x7fELF") {
        if let Some(p) = parse_elf(path, head) {
            parsed = p;
            has_format = declared_format(&parsed);
        }
    } else if macho_ok(head) {
        if let Some(p) = parse_macho(head) {
            parsed = p;
            has_format = declared_format(&parsed);
        }
    } else if head.starts_with(b"%PDF") {
        parsed = parse_pdf(path, len, head);
        has_format = true;
    } else if is_font_sig(head) {
        if let Some(p) = parse_font(path, head) {
            parsed = p;
            has_format = declared_format(&parsed);
        }
    } else if head.starts_with(&[0x1F, 0x8B]) {
        parsed = parse_gzip(path, len, head);
        has_format = true;
    }

    if !has_format && let Some(name) = sig_format(head) {
        parsed.insert(0, pair("format", name));
    }

    let mut pairs = Vec::with_capacity(parsed.len() + 8);
    pairs.push(pair("type", type_label(ext, meta.is_dir())));
    pairs.extend(parsed);
    if !head.is_empty() {
        let n = head.len().min(8);
        pairs.push(pair("signature", hex_bytes(&head[..n])));
    }
    if !meta.is_dir() {
        pairs.push(pair("size", fmt_bytes(len)));
    }
    if let Ok(t) = meta.modified()
        && let Some(s) = fmt_system_time(t)
    {
        pairs.push(pair("modified", s));
    }
    if let Ok(t) = meta.created()
        && let Some(s) = fmt_system_time(t)
    {
        pairs.push(pair("created", s));
    }
    if let Ok(t) = meta.accessed()
        && let Some(s) = fmt_system_time(t)
    {
        pairs.push(pair("accessed", s));
    }
    #[cfg(windows)]
    if let Some(attrs) = windows_attributes(meta) {
        pairs.push(pair("attributes", attrs));
    }
    if let Some(note) = note {
        pairs.push(pair("note", note.clone()));
        return Preview::Meta(MetaPreview {
            pairs,
            note: Some(note),
        });
    }
    Preview::Meta(MetaPreview { pairs, note: None })
}

pub fn empty(meta: &Metadata) -> Preview {
    let mut pairs = vec![
        pair("type", "empty file"),
        pair("format", "empty"),
        pair("size", "0 B"),
    ];
    if let Ok(t) = meta.modified()
        && let Some(s) = fmt_system_time(t)
    {
        pairs.push(pair("modified", s));
    }
    Preview::Meta(MetaPreview { pairs, note: None })
}

pub fn degraded(path: &Path, meta: Option<&Metadata>, reason: &str, ext: &str) -> Preview {
    let mut pairs = vec![
        pair("type", type_label(ext, meta.is_some_and(|m| m.is_dir()))),
        pair("note", reason.to_string()),
    ];
    if let Some(meta) = meta {
        let len = meta.len();
        let head = read_head(path, len.min(META_HEAD)).unwrap_or_default();
        if let Some(name) = sig_format(&head) {
            pairs.push(pair("format", name));
        }
        if let Ok(t) = meta.modified()
            && let Some(s) = fmt_system_time(t)
        {
            pairs.push(pair("modified", s));
        }
        if !meta.is_dir() {
            pairs.push(pair("size", fmt_bytes(len)));
        }
        #[cfg(windows)]
        if let Some(attrs) = windows_attributes(meta) {
            pairs.push(pair("attributes", attrs));
        }
    }
    Preview::Meta(MetaPreview {
        pairs,
        note: Some(reason.to_string()),
    })
}

fn declared_format(pairs: &[MetaPair]) -> bool {
    pairs.iter().any(|p| p.key == "format")
}

// ---------------------------------------------------------------------------
// Signature detection
// ---------------------------------------------------------------------------

pub fn sig_format(head: &[u8]) -> Option<String> {
    if head.starts_with(b"PK\x03\x04")
        || head.starts_with(b"PK\x05\x06")
        || head.starts_with(b"PK\x07\x08")
    {
        return Some("zip".to_string());
    }
    if head.starts_with(&[0x1F, 0x8B]) {
        return Some("gzip".to_string());
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        let window = &head[..head.len().min(1024)];
        if window.windows(4).any(|w| w == b"webm") {
            return Some("webm".to_string());
        }
        if window.windows(8).any(|w| w == b"matroska") {
            return Some("matroska".to_string());
        }
        return Some("ebml".to_string());
    }
    if head.starts_with(b"BZh") {
        return Some(format!(
            "bzip2 level {}",
            head.get(3).map_or(0, |b| b.saturating_sub(b'0')).min(9)
        ));
    }
    if head.starts_with(b"\xfd7zXZ\x00") {
        return Some("xz".to_string());
    }
    if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        return Some("zstd".to_string());
    }
    if head.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        return Some("lz4".to_string());
    }
    if head.starts_with(b"Rar!") {
        return Some(
            if at(head, 6, &[0x01]) {
                "rar v5"
            } else {
                "rar v4"
            }
            .to_string(),
        );
    }
    if head.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some(if head.len() >= 8 {
            format!("7z.{}", head[7])
        } else {
            "7z".to_string()
        });
    }
    if head.starts_with(b"MSCF") {
        return Some("cab".to_string());
    }
    if head.starts_with(b"!<arch>\n") {
        return Some("ar".to_string());
    }
    if head.starts_with(&[0x8E, 0xAD, 0xE8]) {
        return Some("rpm".to_string());
    }
    if head.starts_with(b"SQLite format 3\x00") {
        return Some("sqlite3".to_string());
    }
    if head.starts_with(b"%PDF-") && head.len() >= 8 {
        let ver = String::from_utf8_lossy(&head[5..8]).trim().to_string();
        if !ver.is_empty() && ver.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Some(format!("pdf {ver}"));
        }
        return Some("pdf".to_string());
    }
    if head.starts_with(b"fLaC") {
        return Some("flac".to_string());
    }
    if head.starts_with(b"OggS") {
        return Some("ogg".to_string());
    }
    if head.starts_with(b"RIFF") && head.len() >= 12 {
        return Some(
            match &head[8..12] {
                b"WAVE" => "wav",
                b"AVI " => "avi",
                b"WEBP" => "webp",
                _ => "riff",
            }
            .to_string(),
        );
    }
    if at(head, 4, b"ftyp") && head.len() >= 12 {
        let brand = String::from_utf8_lossy(&head[8..12]).trim().to_string();
        return Some(format!("mp4 ftyp {brand}"));
    }
    if head.starts_with(b"MSWIM\x00\x00\x00") {
        return Some("wim".to_string());
    }
    if head.starts_with(b"conectix") {
        return Some("vhd".to_string());
    }
    if head.starts_with(b"vhdx") {
        return Some("vhdx".to_string());
    }
    if head.starts_with(b"bplist00") && head.len() >= 9 {
        return Some(format!(
            "binary plist {}",
            String::from_utf8_lossy(&head[8..9])
        ));
    }
    if head.starts_with(b"\x00asm") {
        return Some("wasm".to_string());
    }
    if at(head, 0x8001, b"CD001") {
        return Some("iso9660".to_string());
    }
    if at(head, 257, b"ustar") {
        return Some("tar".to_string());
    }
    if head.starts_with(b"!BDN") {
        return Some("outlook pst".to_string());
    }
    if at(head, 60, b"BOOKMOBI") || head.starts_with(b"BOOKMOBI") {
        return Some("mobi".to_string());
    }
    if head.starts_with(b"wOFF") {
        return Some("woff".to_string());
    }
    if head.starts_with(b"wOF2") {
        return Some("woff2".to_string());
    }
    if head.starts_with(b"ttcf") {
        return Some("ttc".to_string());
    }
    if head.starts_with(b"OTTO") {
        return Some("opentype cff".to_string());
    }
    if head.starts_with(&[0x00, 0x01, 0x00, 0x00]) || head.starts_with(b"true") {
        return Some("truetype".to_string());
    }
    if head.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("png".to_string());
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpeg".to_string());
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some("gif".to_string());
    }
    if head.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || head.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        return Some("tiff".to_string());
    }
    if head.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("ico".to_string());
    }
    if head.starts_with(b"BM") {
        return Some("bmp".to_string());
    }
    if head.starts_with(b"MZ") {
        return Some("pe (mz)".to_string());
    }
    if head.starts_with(b"\x7fELF") {
        return Some("elf".to_string());
    }
    if macho_ok(head) {
        return Some("mach-o".to_string());
    }
    if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Some("ole compound".to_string());
    }
    if head.starts_with(b"ID3") {
        return Some("mp3".to_string());
    }
    if head.len() >= 4 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0 {
        return Some("mpeg audio".to_string());
    }
    None
}

pub fn image_signature(head: &[u8]) -> bool {
    head.starts_with(&[0xFF, 0xD8, 0xFF])
        || head.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
        || head.starts_with(b"GIF87a")
        || head.starts_with(b"GIF89a")
        || head.starts_with(&[0x49, 0x49, 0x2A, 0x00])
        || head.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
        || head.starts_with(&[0x00, 0x00, 0x01, 0x00])
        || at(head, 8, b"WEBP")
        || (head.starts_with(b"BM")
            && head.len() >= 18
            && matches!(u32le(head, 14), Some(12 | 40 | 52 | 56 | 64 | 108 | 124)))
}

pub fn type_label(ext: &str, is_dir: bool) -> String {
    if is_dir {
        return "folder".to_string();
    }
    let label = match ext {
        "exe" | "dll" | "msi" | "so" | "dylib" | "efi" | "sys" | "drv" | "jar" | "apk" | "deb"
        | "rpm" => "executable",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp"
        | "epub" | "mobi" => "document",
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "lz4" | "cab"
        | "wim" => "archive",
        "vhd" | "vhdx" | "vmdk" | "img" | "iso" | "dmg" => "disk image",
        "ttf" | "otf" | "ttc" | "woff" | "woff2" | "fon" => "font",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico" | "heic" => {
            "image"
        }
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "mp3" | "wav" | "flac" | "m4a" | "aac"
        | "ogg" | "oga" | "opus" | "wma" => "media",
        "db" | "sqlite" | "sqlite3" => "database",
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "java" | "c" | "cpp" | "h" | "hpp" | "cs"
        | "go" | "rb" | "php" | "swift" | "kt" | "lua" | "sh" | "ps1" | "bat" | "sql" => "text",
        "txt" | "md" | "rtf" | "log" | "out" | "csv" | "json" | "yaml" | "yml" | "toml" | "xml"
        | "ini" | "cfg" | "conf" | "env" | "pem" | "crt" | "svg" => "text",
        "" => "unknown",
        _ => "binary",
    };
    if ext.is_empty() {
        label.to_string()
    } else {
        format!("{label} (.{ext})")
    }
}

// ---------------------------------------------------------------------------
// PE / ELF / Mach-O
// ---------------------------------------------------------------------------

fn parse_pe(path: &Path, head: &[u8]) -> Option<Vec<MetaPair>> {
    if head.len() < 0x40 {
        return None;
    }
    let e_lfanew = u64::from(u32le(head, 0x3C)?);
    if e_lfanew > 64 * 1024 * 1024 {
        return None;
    }
    let hdr = if e_lfanew.saturating_add(64) <= head.len() as u64 {
        head[e_lfanew as usize..].to_vec()
    } else {
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
    let opt = hdr.get(24..)?;
    let opt_magic = u16le(opt, 0)?;
    let format = match opt_magic {
        0x10B => Some("PE32"),
        0x20B => Some("PE32+"),
        0x107 => Some("PE ROM"),
        _ => None,
    };

    let mut pairs = Vec::new();
    if let Some(f) = format {
        pairs.push(pair("format", f));
    }
    pairs.push(pair("arch", machine_name(machine).to_string()));
    pairs.push(pair(
        "kind",
        if chars & 0x2000 != 0 {
            "dll"
        } else {
            "executable"
        }
        .to_string(),
    ));
    if format.is_some() && opt.len() >= 70 {
        if let Some(image_size) = u32le(opt, 56) {
            pairs.push(pair("image size", fmt_bytes(u64::from(image_size))));
        }
        if let Some(subsystem) = u16le(opt, 68) {
            pairs.push(pair("subsystem", subsystem_name(subsystem).to_string()));
        }
    }
    let mut names = Vec::new();
    for i in 0..usize::from(n_sections).min(96) {
        let start = 24usize
            .checked_add(opt_size)?
            .checked_add(i.checked_mul(40)?)?;
        let Some(raw) = hdr.get(start..start + 8) else {
            break;
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
        pairs.push(pair(
            "build time",
            super::util::fmt_unix_utc(u64::from(timestamp)),
        ));
    }
    Some(pairs)
}

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

fn parse_elf(path: &Path, head: &[u8]) -> Option<Vec<MetaPair>> {
    if head.len() < 52 {
        return None;
    }
    let is64 = head[4] == 2;
    let le = head[5] != 2;
    if is64 && head.len() < 64 {
        return None;
    }
    let rd16 = |off: usize| {
        if le {
            u16le(head, off)
        } else {
            u16be(head, off)
        }
    };
    let rd32 = |off: usize| {
        if le {
            u32le(head, off)
        } else {
            u32be(head, off)
        }
    };
    let rd64 = |off: usize| {
        if le {
            u64le(head, off)
        } else {
            u64be(head, off)
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
        if shnum <= 96 && shoff > 0 && (shentsize == 40 || shentsize == 64) && shstrndx < shnum {
            let names = elf_section_names(path, shoff, shentsize, shnum, shstrndx, is64, le);
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

fn elf_section_names(
    path: &Path,
    shoff: u64,
    shentsize: u16,
    shnum: u16,
    shstrndx: u16,
    is64: bool,
    le: bool,
) -> Vec<String> {
    let total = match u64::from(shnum).checked_mul(u64::from(shentsize)) {
        Some(t) => t,
        None => return Vec::new(),
    };
    if total > 16 * 1024 {
        return Vec::new();
    }
    let table = match read_at(path, shoff, total) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let entry = usize::from(shentsize);
    let str_header = match table
        .get(usize::from(shstrndx) * entry..)
        .and_then(|s| s.get(..entry))
    {
        Some(s) => s,
        None => return Vec::new(),
    };
    let (str_off, str_size) = if is64 {
        match (
            if le {
                u64le(str_header, 24)
            } else {
                u64be(str_header, 24)
            },
            if le {
                u64le(str_header, 32)
            } else {
                u64be(str_header, 32)
            },
        ) {
            (Some(o), Some(s)) => (o, s),
            _ => return Vec::new(),
        }
    } else {
        match (
            if le {
                u32le(str_header, 16)
            } else {
                u32be(str_header, 16)
            },
            if le {
                u32le(str_header, 20)
            } else {
                u32be(str_header, 20)
            },
        ) {
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
        let Some(header) = table.get(i * entry..i * entry + 4) else {
            break;
        };
        let Some(name_off) = (if le {
            u32le(header, 0)
        } else {
            u32be(header, 0)
        }) else {
            break;
        };
        let name = strtab
            .get(name_off as usize..)
            .map(|s| {
                let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
                String::from_utf8_lossy(&s[..end]).trim().to_string()
            })
            .unwrap_or_default();
        if !name.is_empty() {
            names.push(name);
        }
        if names.len() >= 12 {
            break;
        }
    }
    names
}

fn macho_ok(head: &[u8]) -> bool {
    if head.len() < 4 {
        return false;
    }
    let m = [head[0], head[1], head[2], head[3]];
    matches!(
        m,
        [0xFE, 0xED, 0xFA, 0xCE]
            | [0xCE, 0xFA, 0xED, 0xFE]
            | [0xFE, 0xED, 0xFA, 0xCF]
            | [0xCF, 0xFA, 0xED, 0xFE]
            | [0xCA, 0xFE, 0xBA, 0xBE]
            | [0xBE, 0xBA, 0xFE, 0xCA]
    )
}

fn parse_macho(head: &[u8]) -> Option<Vec<MetaPair>> {
    if head.len() < 8 {
        return None;
    }
    let magic = [head[0], head[1], head[2], head[3]];
    if matches!(magic, [0xCA, 0xFE, 0xBA, 0xBE] | [0xBE, 0xBA, 0xFE, 0xCA]) {
        let le = magic == [0xBE, 0xBA, 0xFE, 0xCA];
        let n = if le { u32le(head, 4) } else { u32be(head, 4) }?;
        let mut arcs = Vec::new();
        let mut pos = 8usize;
        for _ in 0..n.min(16) {
            let (cpu, next) = if le {
                let c = u32le(head, pos)?;
                (c, pos + 20)
            } else {
                let c = u32be(head, pos)?;
                (c, pos + 20)
            };
            let name = macho_cpu(cpu);
            if !name.is_empty() {
                arcs.push(name.to_string());
            }
            pos = next;
        }
        let mut pairs = vec![pair("format", "mach-o universal")];
        pairs.push(pair("architectures", n.to_string()));
        if !arcs.is_empty() {
            pairs.push(pair("arch", arcs.join(" ")));
        }
        return Some(pairs);
    }

    let (le, is64) = match magic {
        [0xFE, 0xED, 0xFA, 0xCE] => (false, false),
        [0xCE, 0xFA, 0xED, 0xFE] => (true, false),
        [0xFE, 0xED, 0xFA, 0xCF] => (false, true),
        [0xCF, 0xFA, 0xED, 0xFE] => (true, true),
        _ => return None,
    };
    let needed = if is64 { 32 } else { 28 };
    if head.len() < needed {
        return None;
    }
    let cpu = if le { u32le(head, 4) } else { u32be(head, 4) }?;
    let filetype = if le { u32le(head, 12) } else { u32be(head, 12) }?;
    let ncmds = if le { u32le(head, 16) } else { u32be(head, 16) }?;
    let sizeofcmds = if le { u32le(head, 20) } else { u32be(head, 20) }?;
    let mut pairs = vec![pair(
        "format",
        format!(
            "mach-o {}{}",
            if is64 { "64" } else { "32" },
            if le { " little-endian" } else { "" }
        ),
    )];
    let arch = macho_cpu(cpu);
    if !arch.is_empty() {
        pairs.push(pair("arch", arch.to_string()));
    }
    pairs.push(pair("kind", macho_filetype(filetype).to_string()));
    pairs.push(pair("load commands", ncmds.to_string()));
    if sizeofcmds > 0 {
        pairs.push(pair("commands size", fmt_bytes(u64::from(sizeofcmds))));
    }
    Some(pairs)
}

fn macho_cpu(cpu: u32) -> &'static str {
    match cpu {
        7 => "x86",
        0x0100_0007 => "x86_64",
        12 => "arm",
        0x0100_000C => "arm64",
        18 => "ppc",
        0x0100_0012 => "ppc64",
        _ => "",
    }
}

fn macho_filetype(t: u32) -> &'static str {
    match t {
        1 => "object",
        2 => "executable",
        4 => "core",
        6 => "dylib",
        8 => "bundle",
        11 => "dSYM",
        _ => "other",
    }
}

// ---------------------------------------------------------------------------
// PDF
// ---------------------------------------------------------------------------

fn parse_pdf(path: &Path, len: u64, head: &[u8]) -> Vec<MetaPair> {
    let mut pairs = Vec::new();
    if head.starts_with(b"%PDF-") && head.len() >= 8 {
        let ver = String::from_utf8_lossy(&head[5..8]).trim().to_string();
        if !ver.is_empty() && ver.chars().all(|c| c.is_ascii_digit() || c == '.') {
            pairs.push(pair("format", format!("pdf {ver}")));
        } else {
            pairs.push(pair("format", "pdf"));
        }
    }
    if len <= PDF_SCAN {
        let scan = if (head.len() as u64) >= len {
            head.to_vec()
        } else {
            read_head(path, len).unwrap_or_default()
        };
        let pages = count_pdf_pages(&scan);
        if pages > 0 {
            pairs.push(pair("pages", pages.to_string()));
        }
    }
    pairs
}

fn count_pdf_pages(buf: &[u8]) -> usize {
    let mut count = 0usize;
    let mut rest = buf;
    while let Some(pos) = rest.windows(5).position(|w| w == b"/Type") {
        rest = &rest[pos + 5..];
        let mut i = 0usize;
        while rest
            .get(i)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        {
            i += 1;
        }
        if rest.get(i) == Some(&b'/') && rest.get(i + 1..i + 5).is_some_and(|s| s == b"Page") {
            let boundary_ok = rest.get(i + 5).is_none_or(|b| !b.is_ascii_alphanumeric());
            if boundary_ok {
                count += 1;
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

fn is_font_sig(head: &[u8]) -> bool {
    head.starts_with(b"wOFF")
        || head.starts_with(b"wOF2")
        || head.starts_with(b"ttcf")
        || head.starts_with(b"OTTO")
        || head.starts_with(&[0x00, 0x01, 0x00, 0x00])
        || head.starts_with(b"true")
}

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
    let mut pairs = Vec::new();

    if flavor == b"wOFF" {
        let num_tables = usize::from(u16be(head, 12)?);
        pairs.push(pair("format", "woff"));
        pairs.push(pair("sfnt", sfnt_flavor_name(&head[4..8])));
        let mut tags = Vec::new();
        let mut name_loc: Option<(u64, u64, u64)> = None;
        for i in 0..num_tables.min(64) {
            let base = 44 + i * 20;
            let Some(rec) = head.get(base..base + 20) else {
                break;
            };
            let tag = String::from_utf8_lossy(&rec[0..4]).trim().to_string();
            let off = u32be(rec, 4)?;
            let comp = u32be(rec, 8)?;
            let orig = u32be(rec, 12)?;
            if tag == "name" {
                name_loc = Some((u64::from(off), u64::from(comp), u64::from(orig)));
            }
            if tags.len() < 12 && !tag.is_empty() {
                tags.push(tag);
            }
        }
        pairs.push(pair("tables", format!("{num_tables} ({})", tags.join(" "))));
        if let Some((off, comp, orig)) = name_loc
            && comp == orig
            && let Some(family) = read_name_table_family(path, off, orig)
        {
            pairs.push(pair("family", family));
        }
        return Some(pairs);
    }

    if flavor == b"wOF2" {
        let num_tables = u16be(head, 12)?;
        pairs.push(pair("format", "woff2"));
        pairs.push(pair("sfnt", sfnt_flavor_name(&head[4..8])));
        pairs.push(pair("tables", num_tables.to_string()));
        return Some(pairs);
    }

    let base = if flavor == b"ttcf" {
        let num_fonts = u32be(head, 8)?;
        let first = u64::from(u32be(head, 12)?);
        pairs.push(pair("format", format!("ttc ({num_fonts} fonts)")));
        first
    } else {
        pairs.push(pair("format", sfnt_flavor_name(flavor)));
        0
    };

    let dir = if base == 0 {
        head.to_vec()
    } else {
        read_at(path, base, TABLE_READ_CAP).ok()?
    };
    if dir.len() < 12 {
        return Some(pairs);
    }
    let num_tables = usize::from(u16be(&dir, 4).unwrap_or(0));
    if num_tables == 0 || num_tables > 128 {
        return Some(pairs);
    }
    let mut tags = Vec::new();
    let mut name_loc: Option<(u64, u64)> = None;
    for i in 0..num_tables {
        let base = 12 + i * 16;
        let Some(rec) = dir.get(base..base + 16) else {
            break;
        };
        let tag = String::from_utf8_lossy(&rec[0..4]).to_string();
        let off = u32be(rec, 8)?;
        let size = u32be(rec, 12)?;
        if tag == "name" {
            name_loc = Some((u64::from(off), u64::from(size)));
        }
        if tags.len() < 12 {
            tags.push(tag);
        }
    }
    pairs.push(pair("tables", format!("{num_tables} ({})", tags.join(" "))));
    if let Some((off, size)) = name_loc
        && let Some(family) = read_name_table_family(path, off, size)
    {
        pairs.push(pair("family", family));
    }
    Some(pairs)
}

fn read_name_table_family(path: &Path, off: u64, size: u64) -> Option<String> {
    let buf = read_at(path, off, size.min(16 * 1024)).ok()?;
    if buf.len() < 6 {
        return None;
    }
    let count = usize::from(u16be(&buf, 2)?);
    let string_off = usize::from(u16be(&buf, 4)?);
    let mut best: Option<(u8, String)> = None;
    for i in 0..count.min(256) {
        let Some(rec) = buf.get(6 + i * 12..6 + i * 12 + 12) else {
            break;
        };
        let platform = u16be(rec, 0)?;
        let name_id = u16be(rec, 6)?;
        let len = usize::from(u16be(rec, 8)?);
        let rec_off = usize::from(u16be(rec, 10)?);
        if name_id != 1 && name_id != 4 {
            continue;
        }
        let start = string_off.checked_add(rec_off)?;
        let Some(bytes) = buf.get(start..start.checked_add(len)?) else {
            continue;
        };
        let text = if platform == 3 || platform == 0 {
            decode_utf16_be(bytes)
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        };
        let text = super::util::scrub_inline(&text);
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

fn decode_utf16_be(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

// ---------------------------------------------------------------------------
// Gzip
// ---------------------------------------------------------------------------

fn parse_gzip(path: &Path, len: u64, head: &[u8]) -> Vec<MetaPair> {
    let mut pairs = Vec::new();
    if head.len() < 10 {
        return pairs;
    }
    pairs.push(pair(
        "format",
        if head[2] == 8 {
            "gzip (deflate)"
        } else {
            "gzip"
        },
    ));
    if let Some(mtime) = u32le(head, 4)
        && mtime != 0
    {
        pairs.push(pair(
            "modified",
            super::util::fmt_unix_utc(u64::from(mtime)),
        ));
    }
    if len >= 4
        && let Ok(tail) = read_at(path, len - 4, 4)
        && tail.len() == 4
        && let Some(isize) = u32le(&tail, 0)
    {
        pairs.push(pair("uncompressed", fmt_bytes(u64::from(isize))));
    }
    pairs
}

// ---------------------------------------------------------------------------
// Windows attributes
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn windows_attributes(meta: &Metadata) -> Option<String> {
    use std::os::windows::fs::MetadataExt;
    let a = meta.file_attributes();
    const READONLY: u32 = 0x1;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    const DIRECTORY: u32 = 0x10;
    const ARCHIVE: u32 = 0x20;
    const REPARSE: u32 = 0x400;
    let mut names = Vec::new();
    if a & READONLY != 0 {
        names.push("read-only");
    }
    if a & HIDDEN != 0 {
        names.push("hidden");
    }
    if a & SYSTEM != 0 {
        names.push("system");
    }
    if a & DIRECTORY != 0 {
        names.push("directory");
    }
    if a & ARCHIVE != 0 {
        names.push("archive");
    }
    if a & REPARSE != 0 {
        names.push("reparse point");
    }
    Some(if names.is_empty() {
        "normal".to_string()
    } else {
        names.join(", ")
    })
}
