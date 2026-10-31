//! Windows version resources for executable tiles.
//!
//! This only reads the version resource through `version.dll`; the file is
//! never mapped as an image and never executed. Values are bounded, scrubbed
//! and returned as display lines. On other platforms this is a no-op.

use std::path::Path;

/// Maximum number of display lines returned for a tile body.
const MAX_LINES: usize = 6;
/// Version resources can claim any size; refuse absurd allocations.
const MAX_RESOURCE_BYTES: u32 = 16 * 1024 * 1024;

#[cfg(windows)]
pub fn version_lines(path: &Path) -> Vec<String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut handle = 0u32;
    let size = unsafe { GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle) };
    if size == 0 || size > MAX_RESOURCE_BYTES {
        return Vec::new();
    }
    let mut block = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(wide.as_ptr(), 0, size, block.as_mut_ptr().cast::<c_void>()) }
        == 0
    {
        return Vec::new();
    }

    let mut lang = 0x0409u16;
    let mut codepage = 0x04B0u16;
    let translation = wide_str(r"\VarFileInfo\Translation");
    let mut ptr: *mut c_void = ptr::null_mut();
    let mut len = 0u32;
    if unsafe {
        VerQueryValueW(
            block.as_ptr().cast::<c_void>(),
            translation.as_ptr(),
            &mut ptr,
            &mut len,
        )
    } != 0
        && !ptr.is_null()
        && len >= 4
    {
        // The translation table is an array of little-endian (lang, codepage).
        let raw = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), 4) };
        lang = u16::from_le_bytes([raw[0], raw[1]]);
        codepage = u16::from_le_bytes([raw[2], raw[3]]);
    }

    let mut lines = Vec::new();
    let fields = [
        ("FileDescription", "description"),
        ("ProductName", "product"),
        ("FileVersion", "version"),
        ("CompanyName", "company"),
        ("LegalCopyright", "copyright"),
    ];
    for (field, label) in fields {
        let query = format!(r"\StringFileInfo\{lang:04x}{codepage:04x}\{field}");
        let query = wide_str(&query);
        let mut value: *mut c_void = ptr::null_mut();
        let mut value_len = 0u32;
        if unsafe {
            VerQueryValueW(
                block.as_ptr().cast::<c_void>(),
                query.as_ptr(),
                &mut value,
                &mut value_len,
            )
        } == 0
            || value.is_null()
            || value_len < 2
        {
            continue;
        }
        let units = (value_len as usize / 2).min(512);
        let raw = unsafe { std::slice::from_raw_parts(value.cast::<u16>(), units) };
        let end = raw.iter().position(|u| *u == 0).unwrap_or(raw.len());
        let text = String::from_utf16_lossy(&raw[..end]);
        let text = super::util::scrub_inline(&text);
        if text.is_empty() {
            continue;
        }
        let text = super::util::truncate_chars(&text, 90).0;
        lines.push(format!("{label} · {text}"));
        if lines.len() >= MAX_LINES {
            break;
        }
    }
    lines
}

#[cfg(windows)]
fn wide_str(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

#[cfg(not(windows))]
pub fn version_lines(_path: &Path) -> Vec<String> {
    Vec::new()
}
