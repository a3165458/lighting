use windows::Win32::Foundation::{GetLastError, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    mouse_event, SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
    MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SetCursorPos, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
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
    tracing::info!(
        "touch action={} screen=({},{}) size={}x{} origin=({},{})",
        ev.action,
        px,
        py,
        monitor.width,
        monitor.height,
        monitor.x,
        monitor.y
    );

    place_cursor(px, py);

    match ev.action {
        0 => send_click_part(px, py, MOUSEEVENTF_LEFTDOWN),
        1 => {}
        2 | 3 => send_click_part(px, py, MOUSEEVENTF_LEFTUP),
        4 => send_click_part(px, py, MOUSEEVENTF_RIGHTDOWN),
        5 => send_click_part(px, py, MOUSEEVENTF_RIGHTUP),
        _ => {}
    }
}

pub fn map_to_screen(monitor: &DisplayInfo, x: u16, y: u16) -> (i32, i32) {
    let nx = x as f64 / 65535.0;
    let ny = y as f64 / 65535.0;
    let px = (monitor.x as f64 + nx * monitor.width as f64).round() as i32;
    let py = (monitor.y as f64 + ny * monitor.height as f64).round() as i32;
    (px, py)
}

fn place_cursor(px: i32, py: i32) {
    unsafe {
        let mut now = POINT::default();
        let _ = GetCursorPos(&mut now);
        let dx = px - now.x;
        let dy = py - now.y;
        if dx != 0 || dy != 0 {
            send_mouse(dx, dy, MOUSEEVENTF_MOVE, 0);
            mouse_event(MOUSEEVENTF_MOVE, dx, dy, 0, 0);
        }
        if SetCursorPos(px, py).is_err() {
            tracing::warn!("SetCursorPos failed {:?}", GetLastError());
        }
    }
    let (ax, ay) = to_primary_absolute(px, py);
    send_mouse(ax, ay, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE, 0);
    let (vx, vy) = to_virtual_absolute(px, py);
    send_mouse(
        vx,
        vy,
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        0,
    );
    unsafe {
        mouse_event(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            vx,
            vy,
            0,
            0,
        );
        let mut now = POINT::default();
        if GetCursorPos(&mut now).is_ok() {
            if (now.x - px).abs() > 2 || (now.y - py).abs() > 2 {
                tracing::warn!(
                    "cursor still at ({},{}) wanted ({},{})",
                    now.x,
                    now.y,
                    px,
                    py
                );
                let _ = SetCursorPos(px, py);
            } else {
                tracing::info!("cursor now at ({},{})", now.x, now.y);
            }
        }
    }
}

fn send_click_part(px: i32, py: i32, flags: MOUSE_EVENT_FLAGS) {
    place_cursor(px, py);
    send_mouse(0, 0, flags, 0);
    unsafe {
        mouse_event(flags, 0, 0, 0, 0);
    }
}

fn to_primary_absolute(px: i32, py: i32) -> (i32, i32) {
    unsafe {
        let w = GetSystemMetrics(SM_CXSCREEN).max(1);
        let h = GetSystemMetrics(SM_CYSCREEN).max(1);
        let ax = (px as f64 * 65535.0 / w as f64).round() as i32;
        let ay = (py as f64 * 65535.0 / h as f64).round() as i32;
        (ax.clamp(0, 65535), ay.clamp(0, 65535))
    }
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
    tracing::info!("touch wheel horizontal={horizontal} delta={delta}");
    let flags = if horizontal {
        MOUSEEVENTF_HWHEEL
    } else {
        MOUSEEVENTF_WHEEL
    };
    send_mouse(0, 0, flags, delta as u32);
    unsafe {
        mouse_event(flags, 0, 0, delta, 0);
    }
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
