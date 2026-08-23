use anyhow::{Context, Result};
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

#[derive(Debug, Clone)]
pub struct DisplayInfo {
    /// DXGI output index, matches FFmpeg `ddagrab=output_idx`.
    pub dxgi_index: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

impl DisplayInfo {
    pub fn label(&self) -> String {
        let kind = if self.primary { "主屏" } else { "副屏" };
        format!(
            "#{dxgi} {kind} {w}×{h} @ ({x},{y}) {name}",
            dxgi = self.dxgi_index,
            w = self.width,
            h = self.height,
            x = self.x,
            y = self.y,
            name = self.name
        )
    }
}

pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    match list_via_dxgi() {
        Ok(list) if !list.is_empty() => Ok(list),
        Ok(_) => list_via_gdi(),
        Err(err) => {
            tracing::warn!("DXGI enum failed ({err:#}), fallback to GDI");
            list_via_gdi()
        }
    }
}

fn list_via_dxgi() -> Result<Vec<DisplayInfo>> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().context("CreateDXGIFactory1")?;
        let mut out = Vec::new();
        let mut global = 0u32;
        let mut adapter_idx = 0u32;
        loop {
            let adapter = match factory.EnumAdapters1(adapter_idx) {
                Ok(a) => a,
                Err(_) => break,
            };
            let mut output_idx = 0u32;
            loop {
                let output = match adapter.EnumOutputs(output_idx) {
                    Ok(o) => o,
                    Err(_) => break,
                };
                let desc = output.GetDesc().context("DXGI GetDesc")?;
                let rect = desc.DesktopCoordinates;
                let width = (rect.right - rect.left).max(0) as u32;
                let height = (rect.bottom - rect.top).max(0) as u32;
                if width == 0 || height == 0 {
                    output_idx += 1;
                    continue;
                }
                let name = wchar_to_string(&desc.DeviceName);
                let primary = is_primary_rect(rect);
                out.push(DisplayInfo {
                    dxgi_index: global,
                    name,
                    x: rect.left,
                    y: rect.top,
                    width,
                    height,
                    primary,
                });
                global += 1;
                output_idx += 1;
            }
            adapter_idx += 1;
        }
        Ok(out)
    }
}

struct GdiBag(Vec<DisplayInfo>);

fn list_via_gdi() -> Result<Vec<DisplayInfo>> {
    let mut bag = GdiBag(Vec::new());
    unsafe {
        let _ = EnumDisplayMonitors(HDC::default(), None, Some(enum_proc), LPARAM(&mut bag as *mut _ as isize));
    }
    Ok(bag.0)
}

unsafe extern "system" fn enum_proc(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let bag = &mut *(data.0 as *mut GdiBag);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        let r = info.rcMonitor;
        bag.0.push(DisplayInfo {
            dxgi_index: bag.0.len() as u32,
            name: format!("Display {}", bag.0.len() + 1),
            x: r.left,
            y: r.top,
            width: (r.right - r.left).max(0) as u32,
            height: (r.bottom - r.top).max(0) as u32,
            primary: info.dwFlags & MONITORINFOF_PRIMARY == MONITORINFOF_PRIMARY,
        });
    }
    BOOL(1)
}

fn is_primary_rect(rect: RECT) -> bool {
    let mut bag = GdiBag(Vec::new());
    unsafe {
        let _ = EnumDisplayMonitors(HDC::default(), None, Some(enum_proc), LPARAM(&mut bag as *mut _ as isize));
    }
    bag.0.iter().any(|d| {
        d.primary && d.x == rect.left && d.y == rect.top && d.width == (rect.right - rect.left) as u32
    })
}

fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
