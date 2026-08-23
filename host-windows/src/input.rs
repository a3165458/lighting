use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SetCursorPos, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN,
};

use crate::displays::DisplayInfo;
use crate::protocol::TouchEvent;

pub fn inject_touch(display: &DisplayInfo, ev: TouchEvent) {
    match ev.action {
        6 => send_wheel(false, ev.x as i16 as i32),
        7 => send_wheel(true, ev.x as i16 as i32),
        _ => inject_pointer(display, ev),
    }
}

fn inject_pointer(display: &DisplayInfo, ev: TouchEvent) {
    let nx = ev.x as f64 / 65535.0;
    let ny = ev.y as f64 / 65535.0;
    let px = (display.x as f64 + nx * display.width as f64).round() as i32;
    let py = (display.y as f64 + ny * display.height as f64).round() as i32;
    let (ax, ay) = to_absolute(px, py);

    let w = display.width;
    let h = display.height;
    let dx = display.x;
    let dy = display.y;
    tracing::info!(
        "touch action={} screen=({},{}) abs=({},{}) size={}x{} origin=({},{})",
        ev.action,
        px,
        py,
        ax,
        ay,
        w,
        h,
        dx,
        dy
    );

    unsafe {
        if SetCursorPos(px, py).is_err() {
            tracing::warn!("SetCursorPos({px},{py}) failed {:?}", GetLastError());
        }
    }

    let mut flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
    flags |= match ev.action {
        0 => MOUSEEVENTF_LEFTDOWN,
        1 => MOUSE_EVENT_FLAGS(0),
        2 | 3 => MOUSEEVENTF_LEFTUP,
        4 => MOUSEEVENTF_RIGHTDOWN,
        5 => MOUSEEVENTF_RIGHTUP,
        _ => return,
    };
    send_mouse(ax, ay, flags, 0);
}

fn to_absolute(px: i32, py: i32) -> (i32, i32) {
    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);
        let ax = ((px - vx) as f64 * 65535.0 / (vw - 1).max(1) as f64).round() as i32;
        let ay = ((py - vy) as f64 * 65535.0 / (vh - 1).max(1) as f64).round() as i32;
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
