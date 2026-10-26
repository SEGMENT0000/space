//! IMAGE preview: header dimension check before decode, hard byte/pixel caps,
//! then a downscale whose longest edge is at most 512 px. Output is raw RGBA,
//! never a data URL.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use image::{GenericImageView, ImageReader, Limits};

use super::util::read_head;
use crate::preview::ImagePreview;

pub const IMAGE_BYTES_CAP: u64 = 32 * 1024 * 1024;
pub const IMAGE_PIXELS: u64 = 8_000_000;
pub const IMAGE_EDGE: u32 = 512;
const MAX_DECODE_ALLOC: u64 = 96 * 1024 * 1024;

pub fn build(path: &Path, len: u64) -> Option<ImagePreview> {
    if len == 0 || len > IMAGE_BYTES_CAP {
        return None;
    }
    let raw = read_head(path, len).ok()?;
    thumb_from_bytes(&raw)
}

/// Decodes image bytes (a file head or embedded cover art). Returns `None` for
/// unsupported codecs, over-limit dimensions/allocations and malformed data.
pub fn thumb_from_bytes(raw: &[u8]) -> Option<ImagePreview> {
    if raw.is_empty() || raw.len() as u64 > IMAGE_BYTES_CAP {
        return None;
    }

    let format = ImageReader::new(Cursor::new(raw))
        .with_guessed_format()
        .ok()?
        .format()?;

    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);

    let (source_width, source_height) = {
        let mut probe = ImageReader::new(Cursor::new(raw));
        probe.set_format(format);
        probe.limits(limits.clone());
        probe.into_dimensions().ok()?
    };
    if source_width == 0 || source_height == 0 {
        return None;
    }
    if u64::from(source_width).saturating_mul(u64::from(source_height)) > IMAGE_PIXELS {
        return None;
    }

    let mut reader = ImageReader::new(Cursor::new(raw));
    reader.set_format(format);
    reader.limits(limits);
    let image = reader.decode().ok()?;

    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let (target_w, target_h) = fit(width, height);
    let scaled = if target_w == width && target_h == height {
        image
    } else {
        image.thumbnail_exact(target_w, target_h)
    };
    let rgba = scaled.to_rgba8();
    let (out_w, out_h) = rgba.dimensions();
    let pixels: Arc<[u8]> = rgba.into_raw().into();

    Some(ImagePreview {
        width: out_w,
        height: out_h,
        rgba: pixels,
        mime: format.to_mime_type().to_string(),
        source_width,
        source_height,
    })
}

fn fit(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= IMAGE_EDGE {
        return (width, height);
    }
    if width >= height {
        let h = (u64::from(height) * u64::from(IMAGE_EDGE) / u64::from(width)).max(1);
        (IMAGE_EDGE, u32::try_from(h).unwrap_or(1))
    } else {
        let w = (u64::from(width) * u64::from(IMAGE_EDGE) / u64::from(height)).max(1);
        (u32::try_from(w).unwrap_or(1), IMAGE_EDGE)
    }
}
