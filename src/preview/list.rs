//! LIST previews: folders, zip central directories, uncompressed tar walks,
//! ISO9660 root directories, xlsx/ods sheet names and docx/odt headings.
//!
//! Archives are never extracted. Zip member data is only decompressed for
//! small XML manifests/headings, always through a hard byte cap. Names are
//! scrubbed and every listing is capped at 200 rows.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use zip::ZipArchive;

use super::util::{
    DIR_COUNT_MAX, DIR_HEAD, LIST_CAP, TEXT_CAP, at, display_name, io_reason, read_at,
    scrub_inline, truncate_chars, u16le, u32le,
};
use crate::preview::{ListEntry, ListKind, ListPreview};

const TAR_WALK_MAX: usize = LIST_CAP + 1;

fn entry(name: String, size: Option<u64>, is_dir: bool, method: Option<String>) -> ListEntry {
    ListEntry {
        name: display_name(&name),
        size,
        kind: if is_dir {
            ListKind::Dir
        } else {
            ListKind::File
        },
        method,
    }
}

fn finish(
    mut entries: Vec<ListEntry>,
    mut truncated: bool,
    total: Option<u64>,
    kind: &str,
) -> ListPreview {
    if entries.len() > LIST_CAP {
        truncated = true;
    }
    entries.truncate(LIST_CAP);
    ListPreview {
        kind: kind.to_string(),
        entries,
        truncated,
        total,
    }
}

// ---------------------------------------------------------------------------
// Folder
// ---------------------------------------------------------------------------

pub fn folder(dir: &Path) -> Result<ListPreview, String> {
    let rd = fs::read_dir(dir).map_err(|e| io_reason(&e))?;
    let mut dirs: Vec<ListEntry> = Vec::new();
    let mut files: Vec<(Option<u64>, ListEntry)> = Vec::new();
    let mut total: u64 = 0;
    let mut total_known = true;
    let mut truncated = false;

    for item in rd {
        let dir_entry = match item {
            Ok(e) => e,
            Err(_) => {
                truncated = true;
                continue;
            }
        };
        let ft = match dir_entry.file_type() {
            Ok(t) => t,
            Err(_) => {
                truncated = true;
                continue;
            }
        };
        if ft.is_symlink() {
            continue; // Never follow a link target.
        }
        total += 1;
        if total > DIR_COUNT_MAX {
            total_known = false;
            truncated = true;
            break;
        }
        if dirs.len() + files.len() >= DIR_HEAD {
            truncated = true;
            continue;
        }
        let name = display_name(&dir_entry.file_name().to_string_lossy());
        if ft.is_dir() {
            dirs.push(entry(name, None, true, None));
        } else {
            let size = dir_entry.metadata().ok().map(|m| m.len());
            files.push((size, entry(name, size, false, None)));
        }
    }

    dirs.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    files.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| {
                a.1.name
                    .to_ascii_lowercase()
                    .cmp(&b.1.name.to_ascii_lowercase())
            })
            .then_with(|| a.1.name.cmp(&b.1.name))
    });

    let mut entries = dirs;
    entries.extend(files.into_iter().map(|(_, e)| e));
    if entries.len() > LIST_CAP {
        truncated = true;
    }
    Ok(finish(
        entries,
        truncated,
        if total_known { Some(total) } else { None },
        "folder",
    ))
}

// ---------------------------------------------------------------------------
// Zip / xlsx / ods / docx / odt
// ---------------------------------------------------------------------------

fn zip_open(path: &Path) -> Result<ZipArchive<File>, String> {
    let file = File::open(path).map_err(|e| io_reason(&e))?;
    ZipArchive::new(file).map_err(|e| format!("unreadable zip: {e}"))
}

pub fn zip(path: &Path) -> Result<ListPreview, String> {
    let mut archive = zip_open(path)?;
    let total = archive.len();
    let take = total.min(LIST_CAP);
    let mut entries = Vec::with_capacity(take);
    for i in 0..take {
        let file = match archive.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                if entries.is_empty() {
                    return Err(format!("unreadable zip: {e}"));
                }
                break;
            }
        };
        let is_dir = file.is_dir();
        let method = if is_dir {
            None
        } else {
            Some(match file.compression() {
                zip::CompressionMethod::Stored => "Store".to_string(),
                zip::CompressionMethod::Deflated => "Deflate".to_string(),
                other => other.to_string(),
            })
        };
        entries.push(entry(
            file.name().to_string(),
            if is_dir { None } else { Some(file.size()) },
            is_dir,
            method,
        ));
    }
    Ok(finish(entries, total > LIST_CAP, Some(total as u64), "zip"))
}

/// Decompresses one zip member with a hard output cap, returning
/// `(lossy UTF-8, hit_cap)`. Used only for small XML manifests.
fn zip_part_text(
    archive: &mut ZipArchive<File>,
    part: &str,
    cap: u64,
) -> Result<(String, bool), String> {
    let file = archive
        .by_name(part)
        .map_err(|e| format!("missing {part}: {e}"))?;
    let mut buf = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| format!("unreadable {part}: {e}"))?;
    let hit_cap = buf.len() as u64 > cap;
    buf.truncate(usize::try_from(cap).unwrap_or(usize::MAX));
    Ok((String::from_utf8_lossy(&buf).into_owned(), hit_cap))
}

pub fn sheet(path: &Path, ext: &str) -> Result<ListPreview, String> {
    let mut archive = zip_open(path)?;
    let (part, open, attr) = if ext == "ods" {
        ("content.xml", "<table:table ", "table:name=\"")
    } else {
        ("xl/workbook.xml", "<sheet ", "name=\"")
    };
    let (xml, hit_cap) = zip_part_text(&mut archive, part, TEXT_CAP)?;
    let names = xml_tag_attrs(&xml, open, attr);
    if names.is_empty() {
        return Err("no sheet names found".to_string());
    }
    let total = names.len();
    let entries = names
        .into_iter()
        .map(|name| entry(name, None, false, None))
        .collect();
    Ok(finish(
        entries,
        hit_cap || total > LIST_CAP,
        if hit_cap { None } else { Some(total as u64) },
        "sheet",
    ))
}

fn xml_tag_attrs(xml: &str, open: &str, attr: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = xml;
    while let Some(pos) = rest.find(open) {
        let window_end = pos.saturating_add(320).min(rest.len());
        let window = &rest[pos..window_end];
        if let Some(nq) = window.find(attr) {
            let start = nq + attr.len();
            if let Some(end) = window[start..].find('"') {
                let name = xml_unescape(&window[start..start + end]);
                if !name.trim().is_empty() {
                    names.push(name);
                }
            }
        }
        rest = &rest[pos + open.len()..];
    }
    names
}

pub fn office(path: &Path, ext: &str) -> Result<ListPreview, String> {
    let mut archive = zip_open(path)?;
    let (part, kind) = if ext == "odt" {
        ("content.xml", "odt")
    } else {
        ("word/document.xml", "docx")
    };
    let (xml, hit_cap) = zip_part_text(&mut archive, part, TEXT_CAP)?;
    let headings = if kind == "odt" {
        odt_headings(&xml)
    } else {
        docx_headings(&xml)
    };
    if headings.is_empty() {
        return Err("no headings found".to_string());
    }
    let total = headings.len();
    let entries = headings
        .into_iter()
        .map(|(text, level)| {
            entry(
                text,
                None,
                false,
                Some(match level {
                    Some(l) => format!("Heading {l}"),
                    None => "Heading".to_string(),
                }),
            )
        })
        .collect();
    Ok(finish(
        entries,
        hit_cap || total > LIST_CAP,
        if hit_cap { None } else { Some(total as u64) },
        "office",
    ))
}

fn docx_headings(xml: &str) -> Vec<(String, Option<u8>)> {
    let lower = xml.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(pos) = lower[from..].find("<w:p") {
        let start = from + pos;
        let after = start + 4;
        let next = lower.as_bytes().get(after).copied();
        if !matches!(
            next,
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') | Some(b'/')
        ) {
            from = after;
            continue;
        }
        let Some(close) = lower[start..].find("</w:p>") else {
            break;
        };
        let end = start + close + "</w:p>".len();
        let para = &xml[start..end];
        let para_lower = &lower[start..end];
        let heading = if let Some(ppr) = para_lower.find("</w:ppr>") {
            para_lower[..ppr].contains("heading")
        } else {
            false
        };
        if heading {
            let text = scrub_inline(&xml_plain_text(para));
            if !text.is_empty() {
                let level = heading_level(para_lower);
                let (text, _) = truncate_chars(&text, 200);
                out.push((text, level));
                if out.len() >= LIST_CAP {
                    break;
                }
            }
        }
        from = end;
    }
    out
}

fn odt_headings(xml: &str) -> Vec<(String, Option<u8>)> {
    let lower = xml.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(pos) = lower[from..].find("<text:h") {
        let start = from + pos;
        let after = start + "<text:h".len();
        let next = lower.as_bytes().get(after).copied();
        if !matches!(
            next,
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') | Some(b'/')
        ) {
            from = after;
            continue;
        }
        let Some(close) = lower[start..].find("</text:h>") else {
            break;
        };
        let end = start + close + "</text:h>".len();
        let tag_end = lower[start..]
            .find('>')
            .map(|p| start + p + 1)
            .unwrap_or(end);
        let open_tag = lower.get(start..tag_end).unwrap_or("");
        let level = open_tag.find("outline-level=\"").and_then(|p| {
            let rest = &open_tag[p + "outline-level=\"".len()..];
            rest.find('"').and_then(|q| rest[..q].parse::<u8>().ok())
        });
        let text = scrub_inline(&xml_plain_text(&xml[start..end]));
        if !text.is_empty() {
            let (text, _) = truncate_chars(&text, 200);
            out.push((text, level));
            if out.len() >= LIST_CAP {
                break;
            }
        }
        from = end;
    }
    out
}

fn heading_level(lower_para: &str) -> Option<u8> {
    let p = lower_para.find("heading")?;
    lower_para[p + "heading".len()..]
        .chars()
        .find(|c| c.is_ascii_digit())
        .and_then(|c| c.to_digit(10))
        .and_then(|d| u8::try_from(d).ok())
}

/// Drops tags, turns paragraph/line breaks into spaces, unescapes entities.
fn xml_plain_text(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut i = 0usize;
    let bytes = xml.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            match xml[i..].find('>') {
                Some(end) => {
                    let tag = &xml[i..i + end];
                    let lower = tag.to_ascii_lowercase();
                    let is_break = lower.contains(":tab")
                        || lower.ends_with("tab")
                        || lower.contains(":br")
                        || lower.ends_with("br");
                    if is_break || lower.starts_with("</") {
                        out.push(' ');
                    }
                    i = i + end + 1;
                }
                None => break,
            }
        } else {
            match xml[i..].chars().next() {
                Some(c) => {
                    out.push(c);
                    i += c.len_utf8();
                }
                None => break,
            }
        }
    }
    xml_unescape(&out)
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#10;", " ")
        .replace("&#13;", " ")
}

// ---------------------------------------------------------------------------
// Tar (uncompressed)
// ---------------------------------------------------------------------------

pub fn tar(path: &Path, len: u64) -> Result<ListPreview, String> {
    let mut entries: Vec<ListEntry> = Vec::new();
    let mut offset: u64 = 0;
    let mut pending_name: Option<String> = None;

    while entries.len() < TAR_WALK_MAX {
        if offset.saturating_add(512) > len.saturating_add(512) {
            break;
        }
        let block = match read_at(path, offset, 512) {
            Ok(b) => b,
            Err(e) => {
                if entries.is_empty() {
                    return Err(e);
                }
                break;
            }
        };
        if block.len() < 512 {
            break;
        }
        if block[0] == 0 {
            break;
        }
        if offset == 0 && !tar_checksum_ok(&block) {
            return Err("not a tar archive".to_string());
        }
        let size = match tar_octal(&block[124..136]) {
            Some(v) => v,
            None => break,
        };
        let typeflag = block[156];
        let name = pending_name.take().unwrap_or_else(|| tar_name(&block));
        let data_off = match offset.checked_add(512) {
            Some(v) => v,
            None => break,
        };

        match typeflag {
            b'L' | b'x' => {
                let raw = read_at(path, data_off, size.min(1024)).unwrap_or_default();
                let text = String::from_utf8_lossy(&raw);
                let mut value = text.trim_end_matches('\0').trim().to_string();
                if typeflag == b'x'
                    && let Some(p) = text.find("path=")
                {
                    let v = text[p + 5..].lines().next().unwrap_or("").trim();
                    if !v.is_empty() {
                        value = v.to_string();
                    }
                }
                if !value.is_empty() {
                    pending_name = Some(value);
                }
            }
            _ => {
                if !name.is_empty() {
                    let is_dir = typeflag == b'5';
                    let method = match typeflag {
                        b'5' => None,
                        b'1' | b'2' => Some("symlink".to_string()),
                        _ => Some("Store".to_string()),
                    };
                    let shown = name.trim_end_matches('/').to_string();
                    entries.push(entry(
                        shown,
                        if is_dir { None } else { Some(size) },
                        is_dir,
                        method,
                    ));
                }
            }
        }

        let padded = match size.checked_add(511) {
            Some(v) => v / 512 * 512,
            None => break,
        };
        offset = match data_off.checked_add(padded) {
            Some(v) => v,
            None => break,
        };
        if offset > (1u64 << 40) {
            break;
        }
    }

    if entries.is_empty() {
        return Err("empty or unsupported tar".to_string());
    }
    let truncated = entries.len() >= TAR_WALK_MAX;
    let total = if truncated {
        None
    } else {
        Some(entries.len() as u64)
    };
    Ok(finish(entries, truncated, total, "tar"))
}

fn tar_checksum_ok(block: &[u8]) -> bool {
    if block.len() < 512 {
        return false;
    }
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
        let mut acc: u64 = u64::from(field[0] & 0x7F);
        for &b in &field[1..] {
            acc = acc.checked_mul(256)?.checked_add(u64::from(b))?;
        }
        return Some(acc);
    }
    let text: Vec<u8> = field
        .iter()
        .copied()
        .skip_while(|b| *b == 0 || *b == b' ')
        .take_while(|b| *b != 0 && *b != b' ')
        .collect();
    if text.is_empty() {
        return Some(0);
    }
    std::str::from_utf8(&text)
        .ok()
        .and_then(|s| u64::from_str_radix(s, 8).ok())
}

fn tar_name(block: &[u8]) -> String {
    let base = cstr_field(&block[0..100]);
    let prefix = cstr_field(block.get(345..500).unwrap_or(&[]));
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

// ---------------------------------------------------------------------------
// ISO9660
// ---------------------------------------------------------------------------

pub fn iso(path: &Path, len: u64) -> Result<ListPreview, String> {
    let pvd = read_at(path, 16 * 2048, 2048)?;
    if pvd.len() < 2048 || pvd[0] != 1 || !at(&pvd, 1, b"CD001") {
        return Err("not an iso9660 image".to_string());
    }
    let block_size = u16le(&pvd, 128).filter(|n| *n >= 512).unwrap_or(2048);
    let root = &pvd[156..190];
    let extent = u32le(root, 2).ok_or("bad iso root extent")?;
    let dir_len = u32le(root, 10).ok_or("bad iso root length")?;
    if dir_len == 0 {
        return Err("empty iso root".to_string());
    }
    let read_n = u64::from(dir_len).min(64 * 1024);
    let start = u64::from(extent).saturating_mul(u64::from(block_size));
    if start > len {
        return Err("iso directory outside file".to_string());
    }
    let data = read_at(path, start, read_n)?;
    let full_dir = u64::from(dir_len) == read_n;

    let mut entries = Vec::new();
    let mut total: u64 = 0;
    let mut truncated = !full_dir;
    let mut pos = 0usize;
    while pos < data.len() {
        let rec_len = data[pos] as usize;
        if rec_len == 0 {
            break;
        }
        if rec_len < 34 || pos + rec_len > data.len() {
            break;
        }
        let rec = &data[pos..pos + rec_len];
        let name_len = rec[32] as usize;
        if 33 + name_len <= rec_len {
            let raw = &rec[33..33 + name_len];
            if !raw.is_empty() && raw[0] != 0 && raw[0] != 1 {
                let mut name = String::from_utf8_lossy(raw).into_owned();
                if let Some(p) = name.find(';') {
                    name.truncate(p);
                }
                let name = name.trim_end_matches('.').to_string();
                let is_dir = rec[25] & 0x02 != 0;
                let size = u32le(rec, 10).unwrap_or(0);
                total += 1;
                if entries.len() < LIST_CAP {
                    entries.push(entry(
                        name,
                        if is_dir { None } else { Some(u64::from(size)) },
                        is_dir,
                        None,
                    ));
                } else {
                    truncated = true;
                }
            }
        }
        pos += rec_len;
    }
    if entries.is_empty() {
        return Err("empty iso root".to_string());
    }
    Ok(finish(
        entries,
        truncated || total > LIST_CAP as u64,
        if full_dir { Some(total) } else { None },
        "iso",
    ))
}
