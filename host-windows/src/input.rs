use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
    MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

use crate::displays::DisplayInfo;
use crate::protocol::TouchEvent;

pub fn inject_touch(monitor: &DisplayInfo, ev: TouchEvent) {
    match ev.action {
        6 => send_wheel(false, ev.x as i16 as i32),
        7 => send_wheel(true, ev.x as i16 as i32),
        _ => inject_pointer(monitor, ev),
    }
}

fn inject_pointer(monitor: &DisplayInfo, ev: TouchEvent) {
    let (px, py) = map_to_screen(monitor, ev.x, ev.y);
    tracing::debug!(
        "touch action={} screen=({},{}) size={}x{} origin=({},{})",
        ev.action,
        px,
        py,
        monitor.width,
        monitor.height,
        monitor.x,
        monitor.y
    );
    // GlideX / Sunshine: one absolute virtual-desktop event. Relative MOVE +
    // SetCursorPos + mouse_event used to inject 6–8 packets per sample.
    let (vx, vy) = to_virtual_absolute(px, py);
    let mut flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
    match ev.action {
        0 => flags |= MOUSEEVENTF_LEFTDOWN,
        2 | 3 => flags |= MOUSEEVENTF_LEFTUP,
        4 => flags |= MOUSEEVENTF_RIGHTDOWN,
        5 => flags |= MOUSEEVENTF_RIGHTUP,
        _ => {}
    }
    send_mouse(vx, vy, flags, 0);
}

pub fn map_to_screen(monitor: &DisplayInfo, x: u16, y: u16) -> (i32, i32) {
    let nx = x as f64 / 65535.0;
    let ny = y as f64 / 65535.0;
    let px = (monitor.x as f64 + nx * monitor.width as f64).round() as i32;
    let py = (monitor.y as f64 + ny * monitor.height as f64).round() as i32;
    (px, py)
}

fn to_virtual_absolute(px: i32, py: i32) -> (i32, i32) {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);
        let ax = ((px - vx) as f64 * 65535.0 / vw as f64).round() as i32;
        let ay = ((py - vy) as f64 * 65535.0 / vh as f64).round() as i32;
        (ax.clamp(0, 65535), ay.clamp(0, 65535))
    }
}

fn send_wheel(horizontal: bool, delta: i32) {
    if delta == 0 {
        return;
    }
    tracing::debug!("touch wheel horizontal={horizontal} delta={delta}");
    let flags = if horizontal {
        MOUSEEVENTF_HWHEEL
    } else {
        MOUSEEVENTF_WHEEL
    };
    send_mouse(0, 0, flags, delta as u32);
}

fn send_mouse(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS, data: u32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe {
        let n = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        if n == 0 {
            tracing::warn!("SendInput failed {:?}", GetLastError());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_normalized_touch_onto_selected_display() {
        let display = DisplayInfo {
            dxgi: None,
            name: "HDMI".into(),
            friendly: "HDMI".into(),
            is_virtual: false,
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
            primary: false,
        };
        assert_eq!(map_to_screen(&display, 0, 0), (1920, 0));
        assert_eq!(map_to_screen(&display, 65535, 65535), (3840, 1080));
        assert_eq!(map_to_screen(&display, 32768, 32768), (2880, 540));
    }
}
