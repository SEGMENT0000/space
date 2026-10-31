//! Opt-in Windows shell thumbnails for tile art.
//!
//! This module is only compiled on Windows and is only called when the user
//! turns on "Rich previews (uses Windows)". The shell is asked for an image
//! through `IShellItemImageFactory`, never for a verb, and the file is never
//! opened for execution. Calls run on a worker thread with a hard timeout and
//! a byte-size gate; failure returns `None` so the caller can use the
//! in-process preview.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::mpsc;
use std::time::Duration;

use windows_sys::Win32::Foundation::SIZE;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits, GetObjectW,
    ReleaseDC,
};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::UI::Shell::{
    SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_RESIZETOFIT, SIIGBF_THUMBNAILONLY,
};
use windows_sys::core::{GUID, HRESULT};

use crate::preview::ImagePreview;

/// Files above this size are never sent to the shell thumbnail provider.
pub const RICH_PREVIEW_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// A shell provider that takes longer than this is abandoned (it can never
/// block the UI thread because the call already runs off-thread).
pub const RICH_PREVIEW_TIMEOUT: Duration = Duration::from_millis(1_800);
/// Hard cap on a returned bitmap edge, so a misbehaving provider cannot make
/// us allocate an enormous buffer.
const MAX_EDGE: i32 = 1_024;

const IID_ISHELL_ITEM_IMAGE_FACTORY: GUID = GUID::from_u128(0xbcc18b79_ba16_442f_80c4_8a59c30c463b);

#[repr(C)]
struct ImageFactoryVtbl {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_image: unsafe extern "system" fn(*mut c_void, SIZE, i32, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct ImageFactory {
    vtbl: *const ImageFactoryVtbl,
}

/// Requests a shell thumbnail with a timeout. The call itself is synchronous
/// inside its own worker thread, so a slow shell extension can never stall the
/// UI; the timeout merely stops us waiting for it.
pub fn thumbnail(path: &Path, edge: u32, len: u64) -> Option<ImagePreview> {
    if edge == 0 || len == 0 || len > RICH_PREVIEW_MAX_BYTES {
        return None;
    }
    let path: PathBuf = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("space-shell-thumb".into())
        .spawn(move || {
            let result = unsafe { shell_thumbnail(&path, edge) };
            let _ = tx.send(result);
        })
        .ok()?;
    rx.recv_timeout(RICH_PREVIEW_TIMEOUT).ok().flatten()
}

unsafe fn shell_thumbnail(path: &Path, edge: u32) -> Option<ImagePreview> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let hr = unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    let initialized = hr >= 0;
    let result = unsafe { shell_thumbnail_inner(&wide, edge) };
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

unsafe fn shell_thumbnail_inner(wide: &[u16], edge: u32) -> Option<ImagePreview> {
    let mut item: *mut c_void = ptr::null_mut();
    let hr = unsafe {
        SHCreateItemFromParsingName(
            wide.as_ptr(),
            ptr::null_mut(),
            &IID_ISHELL_ITEM_IMAGE_FACTORY,
            &mut item,
        )
    };
    if hr < 0 || item.is_null() {
        return None;
    }
    let factory = item as *const ImageFactory;
    let vtbl = unsafe { (*factory).vtbl };
    if vtbl.is_null() {
        return None;
    }

    let edge = edge.clamp(16, MAX_EDGE as u32) as i32;
    let mut bitmap: *mut c_void = ptr::null_mut();
    // THUMBNAILONLY rejects a provider that has no real thumbnail instead of
    // accepting the generic file-type icon it would otherwise return.
    let hr = unsafe {
        ((*vtbl).get_image)(
            item,
            SIZE { cx: edge, cy: edge },
            SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK | SIIGBF_THUMBNAILONLY,
            &mut bitmap,
        )
    };
    unsafe { ((*vtbl).release)(item) };
    if hr < 0 || bitmap.is_null() {
        return None;
    }

    let result = unsafe { bitmap_to_rgba(bitmap) };
    unsafe { DeleteObject(bitmap) };
    result
}

unsafe fn bitmap_to_rgba(bitmap: *mut c_void) -> Option<ImagePreview> {
    let mut info = BITMAP::default();
    let got = unsafe {
        GetObjectW(
            bitmap,
            std::mem::size_of::<BITMAP>() as i32,
            &mut info as *mut BITMAP as *mut c_void,
        )
    };
    if got == 0 {
        return None;
    }
    let width = info.bmWidth;
    let height = info.bmHeight.abs();
    if width <= 0 || height <= 0 || width > MAX_EDGE || height > MAX_EDGE {
        return None;
    }
    let stride = width as usize * 4;
    let mut pixels = vec![0u8; stride * height as usize];
    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of_val(&bmi.bmiHeader) as u32;
    bmi.bmiHeader.biWidth = width;
    bmi.bmiHeader.biHeight = -height;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    let dc = unsafe { GetDC(ptr::null_mut()) };
    if dc.is_null() {
        return None;
    }
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr() as *mut c_void,
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    unsafe { ReleaseDC(ptr::null_mut(), dc) };
    if lines == 0 {
        return None;
    }

    let mut any_alpha = false;
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2); // BGRA -> RGBA
        if px[3] != 0 {
            any_alpha = true;
        }
    }
    if !any_alpha {
        for px in pixels.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }

    Some(ImagePreview {
        width: width as u32,
        height: height as u32,
        rgba: pixels.into(),
        mime: "image/png".to_string(),
        source_width: width as u32,
        source_height: height as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_requests_are_refused_before_any_shell_call() {
        let path = Path::new("definitely-not-a-file.bin");
        assert!(thumbnail(path, 0, 1_024).is_none());
        assert!(thumbnail(path, 384, 0).is_none());
        assert!(thumbnail(path, 384, RICH_PREVIEW_MAX_BYTES + 1).is_none());
    }
}
