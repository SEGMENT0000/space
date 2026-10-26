//! HEX preview: the first 4 KB as 16-byte rows with offsets and ASCII.

use std::path::Path;

use super::util::{HEX_CAP, hex_bytes, read_head};
use crate::preview::{HexPreview, HexRow};

pub fn build(path: &Path, len: u64) -> Result<HexPreview, String> {
    let raw = read_head(path, len.min(HEX_CAP))?;
    Ok(from_bytes(&raw, len))
}

pub fn from_bytes(raw: &[u8], total: u64) -> HexPreview {
    let mut rows = Vec::with_capacity(raw.len().div_ceil(16));
    for (i, chunk) in raw.chunks(16).enumerate() {
        let mut ascii = String::with_capacity(chunk.len());
        for b in chunk {
            ascii.push(if (0x20..0x7F).contains(b) {
                *b as char
            } else {
                '·'
            });
        }
        rows.push(HexRow {
            offset: (i as u64) * 16,
            hex: hex_bytes(chunk),
            ascii,
        });
    }
    HexPreview { rows, total }
}
