//! TEXT and SVG previews: bounded head read, BOM-aware decoding, line cap and
//! control-character scrubbing. SVG source is additionally stripped of script
//! elements, event-handler attributes and `javascript:` URLs before display.

use std::path::Path;

use super::util::{TEXT_CAP, read_head, scrub_line};
use crate::preview::TextPreview;

const TEXT_LINES: usize = 120;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextClass {
    Utf8,
    Utf16Le,
    Utf16Be,
    Binary,
}

pub fn build(path: &Path, len: u64, is_svg: bool) -> Result<Option<TextPreview>, String> {
    let raw = read_head(path, len.min(TEXT_CAP))?;
    Ok(from_head(&raw, len, is_svg))
}

pub fn from_head(raw: &[u8], len: u64, is_svg: bool) -> Option<TextPreview> {
    let class = classify_text(raw);
    if class == TextClass::Binary {
        return None;
    }

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

    let body = if is_svg { sanitize_svg(&body) } else { body };
    let whole_file = raw.len() as u64 == len;
    let mut lines: Vec<String> = Vec::new();
    let mut total: u64 = 0;
    for line in body.lines() {
        total += 1;
        if lines.len() < TEXT_LINES {
            lines.push(scrub_line(line));
        }
    }
    // A file that is entirely one empty line still deserves one honest row.
    if lines.is_empty() && whole_file && len > 0 && !body.is_empty() {
        lines.push(String::new());
    }
    let truncated = total > TEXT_LINES as u64 || !whole_file;

    Some(TextPreview {
        lines,
        truncated,
        total_lines: if whole_file { Some(total) } else { None },
        encoding: Some(
            match class {
                TextClass::Utf16Le => "utf-16le",
                TextClass::Utf16Be => "utf-16be",
                _ => "utf-8",
            }
            .to_string(),
        ),
        language: None,
    })
}

/// Content sniff used by extension routing and unknown-file fallback.
pub fn classify_text(head: &[u8]) -> TextClass {
    if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return TextClass::Utf8;
    }
    if head.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) || head.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return TextClass::Binary; // UTF-32: not worth a lossy guess.
    }
    if head.starts_with(&[0xFF, 0xFE]) {
        return TextClass::Utf16Le;
    }
    if head.starts_with(&[0xFE, 0xFF]) {
        return TextClass::Utf16Be;
    }

    let sample = &head[..head.len().min(512)];
    if sample.contains(&0) {
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
        if half >= 8 && odd_zeros * 10 >= half * 3 && even_zeros == 0 {
            return TextClass::Utf16Le;
        }
        if half >= 8 && even_zeros * 10 >= half * 3 && odd_zeros == 0 {
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
    if bad.saturating_mul(20) > sample.len() {
        return TextClass::Binary;
    }
    TextClass::Utf8
}

fn decode_utf16(bytes: &[u8], little: bool) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
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

/// Language hint for the UI. Extensions only; no content guessing.
pub fn language(ext: &str) -> Option<String> {
    let lang = match ext {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "jsx",
        "py" => "python",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" => "cpp",
        "cs" => "csharp",
        "go" => "go",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "lua" => "lua",
        "sh" | "bash" | "zsh" => "shell",
        "ps1" => "powershell",
        "bat" | "cmd" => "batch",
        "sql" => "sql",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "vue" => "vue",
        "svelte" => "svelte",
        "dart" => "dart",
        "ex" | "exs" => "elixir",
        "erl" => "erlang",
        "hs" => "haskell",
        "ml" => "ocaml",
        "r" => "r",
        "jl" => "julia",
        "asm" | "s" => "assembly",
        "ini" | "cfg" | "conf" | "properties" => "ini",
        "md" => "markdown",
        "svg" => "svg",
        "csv" => "csv",
        _ => return None,
    };
    Some(lang.to_string())
}

/// Whole-document SVG sanitizer. Script bodies are removed even when minified
/// onto one line; event-handler attributes and javascript: URLs go too. The
/// output is still just source text — it is never rendered as SVG.
fn sanitize_svg(src: &str) -> String {
    let without_scripts = strip_scripts(src);
    let without_handlers = strip_event_handlers(&without_scripts);
    strip_javascript_urls(&without_handlers)
}

fn strip_scripts(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    loop {
        let lower = rest.to_ascii_lowercase();
        let Some(start) = lower.find("<script") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let Some(gt) = lower[start..].find('>') else {
            break; // unterminated opener: drop the remainder
        };
        let after_open = start + gt + 1;
        let Some(close) = lower[after_open..].find("</script") else {
            break; // unterminated script: drop the remainder
        };
        let close_start = after_open + close;
        let Some(close_gt) = lower[close_start..].find('>') else {
            break;
        };
        rest = &rest[close_start + close_gt + 1..];
    }
    out
}

fn strip_event_handlers(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut copied = 0usize;
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        let starts_on = (bytes[i] == b'o' || bytes[i] == b'O')
            && (bytes[i + 1] == b'n' || bytes[i + 1] == b'N')
            && is_attr_name_start(bytes[i + 2])
            && (i == 0 || is_attr_boundary(bytes[i - 1]));
        if starts_on {
            let mut j = i + 2;
            while j < bytes.len() && is_attr_name(bytes[j]) {
                j += 1;
            }
            let mut k = j;
            while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b'=' {
                k += 1;
                while k < bytes.len() && bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                let end = if k < bytes.len() && (bytes[k] == b'"' || bytes[k] == b'\'') {
                    let quote = bytes[k];
                    match bytes[k + 1..].iter().position(|b| *b == quote) {
                        Some(p) => k + 1 + p + 1,
                        None => bytes.len(),
                    }
                } else {
                    let mut e = k;
                    while e < bytes.len() && !bytes[e].is_ascii_whitespace() && bytes[e] != b'>' {
                        e += 1;
                    }
                    e
                };
                out.push_str(&src[copied..i]);
                copied = end;
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&src[copied..]);
    out
}

fn strip_javascript_urls(src: &str) -> String {
    let lower = src.to_ascii_lowercase();
    let mut out = String::with_capacity(src.len());
    let mut copied = 0usize;
    let mut from = 0usize;
    while let Some(pos) = lower[from..].find("javascript:") {
        let start = from + pos;
        out.push_str(&src[copied..start]);
        out.push_str("blocked:");
        copied = start + "javascript:".len();
        from = copied;
    }
    out.push_str(&src[copied..]);
    out
}

fn is_attr_boundary(b: u8) -> bool {
    b.is_ascii_whitespace() || b == b'<' || b == b'/' || b == b'"' || b == b'\''
}

fn is_attr_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_attr_name(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b':'
}
