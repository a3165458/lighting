//! Sample the Windows hardware cursor for the tablet overlay.
//!
//! Position is relative to the captured display origin. Shape is BGRA so the
//! tablet can draw it immediately without waiting on the video GOP.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lighting_host::cursor_wire::{encode_cursor, CursorPacket, MAX_CURSOR_EDGE};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetDIBits, GetObjectW,
    ReleaseDC, SelectObject, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    HBRUSH, HDC,
};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, GetCursorInfo, GetIconInfo, CURSORINFO, CURSOR_SHOWING, DI_NORMAL, ICONINFO,
};

use crate::displays::DisplayInfo;

pub fn spawn_sampler(
    display: DisplayInfo,
    slot: Arc<Mutex<Option<Vec<u8>>>>,
    stop: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
) {
    let _ = std::thread::Builder::new()
        .name("lighting-cursor".into())
        .spawn(move || {
            // ffmpeg is HIGH_PRIORITY_CLASS; a default sampler is starved
            // for 8–15 ms whenever the encoder is busy (pointer lag vs
            // the laptop). Match Sunshine: this thread stays above encode.
            unsafe {
                let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
            }
            lighting_host::annexb::enter_mmcss();
            let mut last_handle = 0isize;
            let mut last_vis = false;
            let mut last_x = i16::MIN;
            let mut last_y = i16::MIN;
            while !stop.load(Ordering::Relaxed) {
                let Some(sample) = sample(&display, &mut last_handle) else {
                    std::thread::sleep(Duration::from_millis(
                        lighting_host::session_policy::cursor_sample_interval_ms(),
                    ));
                    continue;
                };
                let moved = sample.x != last_x || sample.y != last_y || sample.visible != last_vis;
                let shaped = sample.bgra.is_some();
                if moved || shaped {
                    last_x = sample.x;
                    last_y = sample.y;
                    last_vis = sample.visible;
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some(encode_cursor(&sample));
                    }
                    notify.notify_one();
                    // GlideX: while the pointer is moving, sample the next
                    // pose immediately. Sleeping 1 ms here sat every update
                    // on a timer tick next to a 120 Hz panel. Yield so a
                    // 100% GetCursorInfo spin cannot starve ffmpeg.
                    std::thread::yield_now();
                    continue;
                }
                std::thread::sleep(Duration::from_millis(
                    lighting_host::session_policy::cursor_sample_interval_ms(),
                ));
            }
        });
}

fn sample(display: &DisplayInfo, last_handle: &mut isize) -> Option<CursorPacket> {
    unsafe {
        let mut info = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        GetCursorInfo(&mut info).ok()?;
        let showing = info.flags.0 & CURSOR_SHOWING.0 == CURSOR_SHOWING.0;
        let POINT { x, y } = info.ptScreenPos;
        let rel_x = x - display.x;
        let rel_y = y - display.y;
        let on_display = rel_x >= -64
            && rel_y >= -64
            && rel_x < display.width as i32 + 64
            && rel_y < display.height as i32 + 64;
        let visible = showing && on_display;
        if !visible {
            *last_handle = 0;
            return Some(CursorPacket {
                visible: false,
                x: 0,
                y: 0,
                hotspot_x: 0,
                hotspot_y: 0,
                width: 0,
                height: 0,
                bgra: None,
            });
        }
        let handle = info.hCursor.0 as usize as isize;
        let shape_changed = handle != 0 && handle != *last_handle;
        let bgra = if shape_changed {
            grab_shape(info.hCursor)
        } else {
            None
        };
        if bgra.is_some() {
            *last_handle = handle;
        }
        let (hot_x, hotspot_y, width, height, pixels) = match bgra {
            Some(shape) => (
                shape.hot_x,
                shape.hot_y,
                shape.width,
                shape.height,
                Some(shape.bgra),
            ),
            None => (0, 0, 0, 0, None),
        };
        Some(CursorPacket {
            visible: true,
            x: rel_x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            y: rel_y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            hotspot_x: hot_x,
            hotspot_y,
            width,
            height,
            bgra: pixels,
        })
    }
}

struct Shape {
    hot_x: u16,
    hot_y: u16,
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

unsafe fn grab_shape(hcursor: windows::Win32::UI::WindowsAndMessaging::HCURSOR) -> Option<Shape> {
    let mut icon = ICONINFO::default();
    GetIconInfo(hcursor, &mut icon).ok()?;
    let hot_x = icon.xHotspot.min(MAX_CURSOR_EDGE) as u16;
    let hot_y = icon.yHotspot.min(MAX_CURSOR_EDGE) as u16;
    let mut bm = BITMAP::default();
    let size_source = if !icon.hbmColor.is_invalid() {
        icon.hbmColor
    } else {
        icon.hbmMask
    };
    if GetObjectW(
        windows::Win32::Graphics::Gdi::HGDIOBJ::from(size_source),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut _),
    ) == 0
    {
        let _ = DeleteObject(icon.hbmColor);
        let _ = DeleteObject(icon.hbmMask);
        return None;
    }
    let mut w = bm.bmWidth.clamp(1, MAX_CURSOR_EDGE as i32) as u32;
    let mut h = bm.bmHeight.clamp(1, MAX_CURSOR_EDGE as i32 * 2) as u32;
    // Mask-only cursors store AND + XOR stacked.
    if icon.hbmColor.is_invalid() && h >= 2 {
        h /= 2;
    }
    w = w.min(MAX_CURSOR_EDGE);
    h = h.min(MAX_CURSOR_EDGE);

    let hdc_screen = GetDC(HWND::default());
    if hdc_screen.is_invalid() {
        let _ = DeleteObject(icon.hbmColor);
        let _ = DeleteObject(icon.hbmMask);
        return None;
    }
    let hdc = CreateCompatibleDC(hdc_screen);
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let dib = CreateDIBSection(
        hdc,
        &header,
        DIB_RGB_COLORS,
        &mut bits,
        windows::Win32::Foundation::HANDLE::default(),
        0,
    );
    let result = (|| {
        let dib = dib.ok()?;
        if bits.is_null() {
            let _ = DeleteObject(dib);
            return None;
        }
        let old = SelectObject(hdc, windows::Win32::Graphics::Gdi::HGDIOBJ::from(dib));
        let _ = DrawIconEx(
            hdc,
            0,
            0,
            hcursor,
            w as i32,
            h as i32,
            0,
            HBRUSH::default(),
            DI_NORMAL,
        );
        let nbytes = (w as usize).saturating_mul(h as usize).saturating_mul(4);
        let mut bgra = vec![0u8; nbytes];
        std::ptr::copy_nonoverlapping(bits as *const u8, bgra.as_mut_ptr(), nbytes);
        let _ = SelectObject(hdc, old);
        let _ = DeleteObject(dib);
        if bgra.chunks(4).all(|p| p[3] == 0) {
            // Masked cursor: recover alpha from the mask plane.
            fill_alpha_from_mask(&mut bgra, w, h, icon.hbmMask, hdc_screen);
        }
        Some(Shape {
            hot_x,
            hot_y,
            width: w,
            height: h,
            bgra,
        })
    })();
    if !hdc.is_invalid() {
        let _ = DeleteDC(hdc);
    }
    let _ = ReleaseDC(HWND::default(), hdc_screen);
    let _ = DeleteObject(icon.hbmColor);
    let _ = DeleteObject(icon.hbmMask);
    result
}

unsafe fn fill_alpha_from_mask(bgra: &mut [u8], w: u32, h: u32, mask: HBITMAP, hdc_screen: HDC) {
    if mask.is_invalid() {
        // Make the pointer visible rather than fully transparent.
        for px in bgra.chunks_mut(4) {
            if px[0] != 0 || px[1] != 0 || px[2] != 0 {
                px[3] = 255;
            }
        }
        return;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut mask_bits = vec![0u8; (w as usize) * (h as usize) * 4];
    let hdc = CreateCompatibleDC(hdc_screen);
    if hdc.is_invalid() {
        return;
    }
    let _ = GetDIBits(
        hdc,
        mask,
        0,
        h,
        Some(mask_bits.as_mut_ptr() as *mut _),
        &mut info,
        DIB_RGB_COLORS,
    );
    for (dst, src) in bgra.chunks_mut(4).zip(mask_bits.chunks(4)) {
        // Mask 0 = opaque pixel of the cursor.
        dst[3] = if src[0] == 0 && src[1] == 0 && src[2] == 0 {
            255
        } else {
            0
        };
    }
    let _ = DeleteDC(hdc);
}
