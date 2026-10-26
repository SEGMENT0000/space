//! MEDIA preview: header-only container parsing plus embedded cover art.
//! Nothing is played, executed or handed to a shell. Video frames are not
//! decoded in this build; when no cover art exists the preview says so plainly.

use std::path::Path;

use super::images;
use super::util::{fmt_bytes, pair, read_at, read_head, u16le, u32be, u32le, u64be};
use crate::preview::{MediaPreview, MetaPair};

const MEDIA_HEAD: u64 = 512 * 1024;
const ART_CAP: u64 = 4 * 1024 * 1024;
const MOOV_CAP: u64 = 4 * 1024 * 1024;

const VIDEO_EXTS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v", "ts", "m2ts"];

#[derive(Default)]
struct MediaFacts {
    pairs: Vec<MetaPair>,
    duration_ms: Option<u64>,
    w: Option<u32>,
    h: Option<u32>,
}

pub fn is_video(ext: &str) -> bool {
    VIDEO_EXTS.contains(&ext)
}

pub fn build(path: &Path, ext: &str, len: u64) -> MediaPreview {
    let kind = if is_video(ext) { "video" } else { "audio" };
    let mime = mime_for(ext).to_string();
    let head = match read_head(path, len.min(MEDIA_HEAD)) {
        Ok(h) => h,
        Err(e) => {
            return MediaPreview {
                kind: kind.to_string(),
                mime,
                thumbnail: None,
                pairs: vec![pair("size", fmt_bytes(len))],
                note: Some(format!("header unavailable: {e}")),
            };
        }
    };

    let mut facts = MediaFacts::default();
    let mut moov: Option<Vec<u8>> = None;
    match ext {
        "mp4" | "m4v" | "mov" | "m4a" => {
            facts = parse_mp4(&head);
            moov = mp4_moov(path, len, &head);
            if let Some(m) = &moov {
                mp4_header_facts(m, &mut facts);
            }
        }
        "mkv" | "webm" => facts = parse_mkv(&head),
        "avi" => facts = parse_avi(&head),
        "wav" => facts = parse_wav(&head),
        "flac" => facts = parse_flac(&head),
        "mp3" | "aac" => facts = parse_mp3(&head),
        "ogg" | "oga" | "opus" => facts = parse_ogg(&head),
        _ => {}
    }

    let cover = match ext {
        "mp3" => id3_cover(path, &head),
        "flac" => flac_cover(path, &head),
        "mp4" | "m4v" | "mov" | "m4a" => moov.as_deref().and_then(mp4_cover),
        _ => None,
    };
    let cover_found = cover.is_some();
    let thumbnail = cover.and_then(|bytes| images::thumb_from_bytes(&bytes));

    let mut pairs = Vec::with_capacity(facts.pairs.len() + 3);
    pairs.push(pair("size", fmt_bytes(len)));
    if !facts.pairs.iter().any(|p| p.key == "format")
        && let Some(name) = super::meta::sig_format(&head)
    {
        pairs.push(pair("format", name));
    }
    if let (Some(w), Some(h)) = (facts.w, facts.h)
        && !facts.pairs.iter().any(|p| p.key == "dimensions")
    {
        pairs.push(pair("dimensions", format!("{w} × {h}")));
    }
    pairs.extend(facts.pairs);

    let note = if thumbnail.is_some() {
        None
    } else if cover_found {
        Some("embedded cover art was found but could not be decoded".to_string())
    } else if kind == "video" {
        Some(
            "No video thumbnail: frame decoding and the Windows shell thumbnail are not enabled \
             in this build."
                .to_string(),
        )
    } else if matches!(ext, "mp3" | "flac" | "m4a" | "mp4" | "m4v" | "mov") {
        Some("No embedded cover art found in the readable header region.".to_string())
    } else {
        Some("Embedded cover art is not extracted for this format in this build.".to_string())
    };

    MediaPreview {
        kind: kind.to_string(),
        mime,
        thumbnail,
        pairs,
        note,
    }
}

fn mime_for(ext: &str) -> &'static str {
    match ext {
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "avi" => "video/x-msvideo",
        "ts" | "m2ts" => "video/mp2t",
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

fn sane_ms(ms: f64) -> Option<u64> {
    if ms.is_finite() && (0.0..=315_576_000_000_000.0).contains(&ms) {
        Some(ms as u64)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// MP4 / M4A / MOV
// ---------------------------------------------------------------------------

fn parse_mp4(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    if head.len() >= 16 && &head[4..8] == b"ftyp" {
        let brand = String::from_utf8_lossy(&head[8..12]).trim().to_string();
        facts
            .pairs
            .push(pair("format", format!("mp4 ftyp {brand}")));
        let box_size = (u32be(head, 0).unwrap_or(16) as usize).min(head.len());
        let mut compatible = Vec::new();
        let mut pos = 16usize;
        while pos + 4 <= box_size {
            let brand = String::from_utf8_lossy(&head[pos..pos + 4])
                .trim()
                .to_string();
            if !brand.is_empty() {
                compatible.push(brand);
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

fn mp4_header_facts(buf: &[u8], facts: &mut MediaFacts) {
    if facts.duration_ms.is_none()
        && let Some(idx) = buf.windows(4).position(|w| w == b"mvhd")
    {
        let p = idx + 4;
        let version = *buf.get(p).unwrap_or(&0);
        let (ts_off, dur_off, wide) = if version == 1 {
            (20usize, 24usize, true)
        } else {
            (12usize, 16usize, false)
        };
        if let Some(timescale) = u32be(buf, p + ts_off) {
            let duration = if wide {
                u64be(buf, p + dur_off)
            } else {
                u32be(buf, p + dur_off).map(u64::from)
            };
            if let Some(duration) = duration
                && timescale > 0
                && duration > 0
            {
                let secs = duration as f64 / f64::from(timescale);
                if let Some(ms) = sane_ms(secs * 1000.0) {
                    facts.duration_ms = Some(ms);
                    facts.pairs.push(pair("duration", format!("{secs:.1} s")));
                }
            }
        }
    }
    if facts.w.is_none()
        && let Some(idx) = buf.windows(4).position(|w| w == b"tkhd")
    {
        let p = idx + 4;
        let version = *buf.get(p).unwrap_or(&0);
        let (w_off, h_off) = if version == 1 {
            (84usize, 88usize)
        } else {
            (76usize, 80usize)
        };
        let w = buf.get(p + w_off..p + w_off + 4).and_then(|s| u32be(s, 0));
        let h = buf.get(p + h_off..p + h_off + 4).and_then(|s| u32be(s, 0));
        if let (Some(w), Some(h)) = (w, h)
            && w >= 0x1_0000
            && h >= 0x1_0000
        {
            facts.w = Some(w >> 16);
            facts.h = Some(h >> 16);
        }
    }
}

/// Locates the `moov` box without reading media data. Uses the head already in
/// memory when possible; otherwise one bounded top-level box walk.
fn mp4_moov(path: &Path, len: u64, head: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0u64;
    for _ in 0..64 {
        if off.saturating_add(8) > len {
            break;
        }
        let hdr = if off.saturating_add(16) <= head.len() as u64 {
            head[off as usize..off as usize + 16].to_vec()
        } else {
            read_at(path, off, 16).ok()?
        };
        if hdr.len() < 8 {
            break;
        }
        let mut size = u64::from(u32be(&hdr, 0).unwrap_or(0));
        let kind = &hdr[4..8];
        let mut body = off + 8;
        if size == 1 {
            let wide = if off.saturating_add(16) <= head.len() as u64 {
                u64be(head, off as usize + 8)
            } else {
                read_at(path, off + 8, 8).ok().and_then(|b| {
                    b.get(..8).map(|s| {
                        let mut a = [0u8; 8];
                        a.copy_from_slice(s);
                        u64::from_be_bytes(a)
                    })
                })
            };
            size = wide?;
            body = off + 16;
        } else if size == 0 {
            size = len.saturating_sub(off);
        }
        if size < 8 {
            break;
        }
        if kind == b"moov" {
            let take = size.min(MOOV_CAP);
            if body.saturating_add(take) <= head.len() as u64 {
                return Some(head[body as usize..(body + take) as usize].to_vec());
            }
            return read_at(path, body, take).ok();
        }
        let next = off.saturating_add(size);
        if next <= off || next >= len {
            break;
        }
        off = next;
    }
    None
}

fn mp4_cover(moov: &[u8]) -> Option<Vec<u8>> {
    let mut from = 0usize;
    while from + 4 <= moov.len() {
        let pos = moov[from..].windows(4).position(|w| w == b"covr")?;
        let tag = from + pos;
        if tag < 4 {
            from = tag + 4;
            continue;
        }
        let covr_start = tag - 4;
        let size = u32be(moov, covr_start)? as usize;
        if size < 8 {
            from = tag + 4;
            continue;
        }
        let covr_end = covr_start.saturating_add(size).min(moov.len());
        let mut p = tag + 4;
        while p + 8 <= covr_end {
            let dsize = u32be(moov, p)? as usize;
            if dsize < 8 {
                break;
            }
            let dend = p.saturating_add(dsize).min(covr_end);
            if &moov[p + 4..p + 8] == b"data" && p + 12 <= dend {
                return Some(moov[p + 12..dend].to_vec());
            }
            let next = p.saturating_add(dsize);
            if next <= p {
                break;
            }
            p = next;
        }
        from = tag + 4;
    }
    None
}

// ---------------------------------------------------------------------------
// Matroska / WebM
// ---------------------------------------------------------------------------

fn ebml_element(buf: &[u8], pos: usize) -> Option<(u64, u64, usize, bool)> {
    let (id, id_len) = ebml_vint(buf, pos)?;
    let (size, size_len, unknown) = ebml_size(buf, pos + id_len)?;
    Some((id, size, id_len + size_len, unknown))
}

fn ebml_vint(buf: &[u8], pos: usize) -> Option<(u64, usize)> {
    let first = *buf.get(pos)?;
    if first == 0 {
        return None;
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

fn ebml_size(buf: &[u8], pos: usize) -> Option<(u64, usize, bool)> {
    let (raw, len) = ebml_vint(buf, pos)?;
    let value = raw & !(1u64 << (7 * len));
    let unknown = value == (1u64 << (7 * len)) - 1;
    Some((value, len, unknown))
}

fn ebml_scan<F: FnMut(u64, usize, usize)>(buf: &[u8], start: usize, end: usize, mut f: F) {
    let mut pos = start.min(end);
    for _ in 0..256 {
        if pos >= end {
            break;
        }
        let Some((id, size, hdr, unknown)) = ebml_element(buf, pos) else {
            break;
        };
        let data = pos + hdr;
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
            break;
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
    let window = &head[..head.len().min(1024)];
    let format = if window.windows(4).any(|w| w == b"webm") {
        "webm"
    } else if window.windows(8).any(|w| w == b"matroska") {
        "matroska"
    } else {
        "ebml"
    };
    facts.pairs.push(pair("format", format));

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

    let mut scale_ns = 1_000_000u64;
    let mut ticks: Option<f64> = None;
    ebml_scan(head, seg_start, seg_end, |id, data, end| match id {
        0x1549_A966 => {
            ebml_scan(head, data, end, |iid, idata, iend| match iid {
                0x002A_D7B1 => {
                    scale_ns = ebml_uint(head, idata, iend)
                        .filter(|v| *v > 0)
                        .unwrap_or(scale_ns)
                }
                0x4489 => ticks = ebml_float(head, idata, iend),
                _ => {}
            });
        }
        0x1654_AE6B => {
            ebml_scan(head, data, end, |tid, tdata, tend| {
                if tid != 0xAE {
                    return;
                }
                let mut video = false;
                ebml_scan(head, tdata, tend, |cid, cdata, cend| match cid {
                    0x83 => video = ebml_uint(head, cdata, cend) == Some(1),
                    0xE0 if video => ebml_scan(head, cdata, cend, |vid, vdata, vend| match vid {
                        0xB0 => {
                            facts.w =
                                ebml_uint(head, vdata, vend).and_then(|v| u32::try_from(v).ok())
                        }
                        0xBA => {
                            facts.h =
                                ebml_uint(head, vdata, vend).and_then(|v| u32::try_from(v).ok())
                        }
                        _ => {}
                    }),
                    _ => {}
                });
            });
        }
        _ => {}
    });

    if let Some(t) = ticks
        && let Some(ms) = sane_ms(t * scale_ns as f64 / 1_000_000.0)
    {
        facts.duration_ms = Some(ms);
        facts
            .pairs
            .push(pair("duration", format!("{:.1} s", ms as f64 / 1000.0)));
    }
    facts
}

// ---------------------------------------------------------------------------
// AVI / WAV / FLAC / MP3 / OGG
// ---------------------------------------------------------------------------

fn parse_avi(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    facts.pairs.push(pair("format", "avi"));
    let Some(idx) = head.windows(4).position(|w| w == b"avih") else {
        return facts;
    };
    let body = idx + 8;
    let micro = u32le(head, body).unwrap_or(0);
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

fn parse_wav(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    facts.pairs.push(pair("format", "wav"));
    let mut pos = 12usize;
    let mut byte_rate = 0u32;
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
            facts.pairs.push(pair(
                "codec",
                match codec {
                    1 => "pcm",
                    3 => "pcm float",
                    0xFFFE => "extensible",
                    _ => "unknown",
                },
            ));
            facts.pairs.push(pair("channels", channels.to_string()));
            facts.pairs.push(pair("sample rate", format!("{rate} Hz")));
            facts.pairs.push(pair("bit depth", bits.to_string()));
        } else if id == b"data" {
            data_size = Some(size as u64);
            break;
        }
        let next = body.saturating_add(size).saturating_add(size & 1);
        if next <= pos {
            break;
        }
        pos = next;
    }
    if let Some(ds) = data_size
        && byte_rate > 0
    {
        let secs = ds as f64 / f64::from(byte_rate);
        facts.duration_ms = sane_ms(secs * 1000.0);
        facts.pairs.push(pair("duration", format!("{secs:.1} s")));
    }
    facts
}

fn parse_flac(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    if head.len() < 26 {
        return facts;
    }
    facts.pairs.push(pair("format", "flac"));
    let b = &head[18..26];
    let rate = (u32::from(b[0]) << 12) | (u32::from(b[1]) << 4) | (u32::from(b[2]) >> 4);
    let channels = ((b[2] >> 1) & 0b111) + 1;
    let bits = (((b[2] & 1) << 4) | (b[3] >> 4)) + 1;
    let total_samples =
        ((u64::from(b[3]) & 0x0F) << 32) | u64::from(u32::from_be_bytes([b[4], b[5], b[6], b[7]]));
    if rate > 0 {
        facts.pairs.push(pair("sample rate", format!("{rate} Hz")));
        facts.pairs.push(pair("channels", channels.to_string()));
        facts.pairs.push(pair("bit depth", bits.to_string()));
        if total_samples > 0 {
            let secs = total_samples as f64 / f64::from(rate);
            facts.duration_ms = sane_ms(secs * 1000.0);
            facts.pairs.push(pair("duration", format!("{secs:.1} s")));
        }
    }
    facts
}

fn parse_mp3(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    let mut frame_off = 0usize;
    if head.starts_with(b"ID3") && head.len() >= 10 {
        let version = head[3];
        facts
            .pairs
            .push(pair("format", format!("mp3 (id3v2.{version})")));
        let tag_size = synchsafe(&head[6..10]).unwrap_or(0);
        facts
            .pairs
            .push(pair("id3 tag", fmt_bytes(u64::from(tag_size))));
        frame_off = 10 + tag_size as usize;
    } else {
        facts.pairs.push(pair("format", "mpeg audio"));
    }
    let sync_at = if head.len() >= frame_off + 4 && head[frame_off] == 0xFF {
        Some(frame_off)
    } else {
        head[..head.len().min(4096)]
            .windows(2)
            .position(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0)
    };
    if let Some(off) = sync_at
        && let Some(b1) = head.get(off + 1).copied()
    {
        let b2 = head.get(off + 2).copied().unwrap_or(0);
        let b3 = head.get(off + 3).copied().unwrap_or(0);
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
            let idx = usize::from((b2 >> 2) & 0b11);
            if let Some(rate) = rates.get(idx) {
                facts.pairs.push(pair("mpeg", format!("{ver} {layer}")));
                facts.pairs.push(pair("sample rate", format!("{rate} Hz")));
                facts.pairs.push(pair(
                    "channels",
                    match b3 >> 6 {
                        0 => "joint stereo",
                        1 => "stereo",
                        2 => "dual channel",
                        _ => "mono",
                    },
                ));
            }
        }
    }
    facts
}

fn parse_ogg(head: &[u8]) -> MediaFacts {
    let mut facts = MediaFacts::default();
    facts.pairs.push(pair("format", "ogg"));
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
    if let Some(codec) = codec {
        facts.pairs.push(pair("codec", codec));
    }
    facts
}

// ---------------------------------------------------------------------------
// Embedded cover art
// ---------------------------------------------------------------------------

fn synchsafe(b: &[u8]) -> Option<u32> {
    if b.len() < 4 {
        return None;
    }
    Some(
        (u32::from(b[0] & 0x7F) << 21)
            | (u32::from(b[1] & 0x7F) << 14)
            | (u32::from(b[2] & 0x7F) << 7)
            | u32::from(b[3] & 0x7F),
    )
}

fn id3_cover(path: &Path, head: &[u8]) -> Option<Vec<u8>> {
    if head.len() < 10 || !head.starts_with(b"ID3") {
        return None;
    }
    let version = head[3];
    let flags = head[5];
    let tag_size = synchsafe(&head[6..10])? as usize;
    if tag_size < 10 || tag_size as u64 > ART_CAP {
        return None;
    }
    let tag = if 10usize.checked_add(tag_size)? <= head.len() {
        head[10..10 + tag_size].to_vec()
    } else {
        read_at(path, 10, tag_size as u64).ok()?
    };
    let mut pos = 0usize;
    if flags & 0x40 != 0 && tag.len() >= 4 {
        let ext = u32::from_be_bytes([tag[0], tag[1], tag[2], tag[3]]) as usize;
        pos = 4usize.saturating_add(ext);
    }
    while pos + 6 <= tag.len() {
        if tag[pos] == 0 {
            break;
        }
        let (id_len, size, header) = if version == 2 {
            let id = &tag[pos..pos + 3];
            let size = (usize::from(tag[pos + 3]) << 16)
                | (usize::from(tag[pos + 4]) << 8)
                | usize::from(tag[pos + 5]);
            (id, size, 6usize)
        } else {
            if pos + 10 > tag.len() {
                break;
            }
            let id = &tag[pos..pos + 4];
            let size = if version >= 4 {
                synchsafe(&tag[pos + 4..pos + 8])? as usize
            } else {
                u32::from_be_bytes([tag[pos + 4], tag[pos + 5], tag[pos + 6], tag[pos + 7]])
                    as usize
            };
            (id, size, 10usize)
        };
        let data_start = pos.checked_add(header)?;
        let data_end = data_start.checked_add(size)?;
        if data_end > tag.len() {
            break;
        }
        let is_pic = (version == 2 && id_len == b"PIC") || (version != 2 && id_len == b"APIC");
        if is_pic && let Some(cover) = parse_apic(&tag[data_start..data_end], version == 2) {
            return Some(cover);
        }
        pos = data_end;
    }
    None
}

fn parse_apic(frame: &[u8], v22: bool) -> Option<Vec<u8>> {
    let encoding = *frame.first()?;
    let mut pos = 1usize;
    if v22 {
        pos = pos.checked_add(3)?; // 3-character image format
    } else {
        let end = frame.get(pos..)?.iter().position(|b| *b == 0)?;
        pos = pos.checked_add(end + 1)?;
    }
    pos = pos.checked_add(1)?; // picture type
    if pos > frame.len() {
        return None;
    }
    if encoding == 1 || encoding == 2 {
        let mut i = pos;
        let mut found = None;
        while i + 1 < frame.len() {
            if frame[i] == 0 && frame[i + 1] == 0 {
                found = Some(i + 2);
                break;
            }
            i += 2;
        }
        pos = found?;
    } else {
        let end = frame.get(pos..)?.iter().position(|b| *b == 0)?;
        pos = pos.checked_add(end + 1)?;
    }
    let data = frame.get(pos..)?;
    if data.is_empty() || data.len() as u64 > ART_CAP {
        return None;
    }
    Some(data.to_vec())
}

fn flac_cover(path: &Path, head: &[u8]) -> Option<Vec<u8>> {
    if !head.starts_with(b"fLaC") {
        return None;
    }
    let mut pos = 4usize;
    for _ in 0..128 {
        let hdr: [u8; 4] = if pos + 4 <= head.len() {
            [head[pos], head[pos + 1], head[pos + 2], head[pos + 3]]
        } else {
            let raw = read_at(path, pos as u64, 4).ok()?;
            if raw.len() < 4 {
                return None;
            }
            [raw[0], raw[1], raw[2], raw[3]]
        };
        let last = hdr[0] & 0x80 != 0;
        let block_type = hdr[0] & 0x7F;
        let block_size =
            (usize::from(hdr[1]) << 16) | (usize::from(hdr[2]) << 8) | usize::from(hdr[3]);
        let data_pos = pos.checked_add(4)?;
        if block_type == 6 {
            if block_size as u64 > ART_CAP {
                return None;
            }
            let block = if data_pos.checked_add(block_size)? <= head.len() {
                head[data_pos..data_pos + block_size].to_vec()
            } else {
                read_at(path, data_pos as u64, block_size as u64).ok()?
            };
            return parse_flac_picture(&block);
        }
        pos = data_pos.checked_add(block_size)?;
        if last {
            break;
        }
    }
    None
}

fn parse_flac_picture(block: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 4usize; // picture type
    let mime_len = u32be(block, pos)? as usize;
    pos = pos.checked_add(4 + mime_len)?;
    let desc_len = u32be(block, pos)? as usize;
    pos = pos.checked_add(4 + desc_len)?;
    pos = pos.checked_add(16)?; // width, height, depth, colors
    let data_len = u32be(block, pos)? as usize;
    pos = pos.checked_add(4)?;
    let end = pos.checked_add(data_len)?.min(block.len());
    let data = block.get(pos..end)?;
    if data.is_empty() || data.len() as u64 > ART_CAP {
        return None;
    }
    Some(data.to_vec())
}
