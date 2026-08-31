//! Capture targets and Windows projection helpers (Win+P equivalents).

use anyhow::{Context, Result};
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::Duration;

use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW, HDC, HMONITOR,
    MONITORINFO,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::PCWSTR;

use lighting_host::view::{looks_virtual_display, ShareMode};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct DisplayInfo {
    /// DXGI output index, matches FFmpeg `ddagrab=output_idx`.
    pub dxgi_index: u32,
    pub name: String,
    pub friendly: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub is_virtual: bool,
}

impl DisplayInfo {
    pub fn label(&self) -> String {
        let kind = if self.primary {
            "主屏"
        } else if self.is_virtual {
            "虚拟屏"
        } else {
            "副屏"
        };
        let title = if self.friendly.is_empty() {
            self.name.clone()
        } else {
            self.friendly.clone()
        };
        format!(
            "#{dxgi} {kind} {w}×{h} · {title}",
            dxgi = self.dxgi_index,
            w = self.width,
            h = self.height,
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

/// Apply the closest Win+P mode via `DisplaySwitch.exe`.
pub fn apply_project_mode(mode: ShareMode) -> Result<()> {
    let arg = mode.display_switch_arg();
    let status = Command::new("DisplaySwitch.exe")
        .arg(arg)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("启动 DisplaySwitch.exe 失败")?;
    if !status.success() {
        anyhow::bail!("DisplaySwitch {arg} 返回 {status}");
    }
    // Windows needs a beat to rebuild the desktop topology.
    std::thread::sleep(Duration::from_millis(900));
    Ok(())
}

pub fn pick_display_index(displays: &[DisplayInfo], mode: ShareMode) -> Option<usize> {
    match mode {
        ShareMode::Mirror => displays.iter().position(|d| d.primary).or(Some(0).filter(|_| !displays.is_empty())),
        ShareMode::Extend | ShareMode::External => displays
            .iter()
            .position(|d| d.is_virtual && !d.primary)
            .or_else(|| displays.iter().position(|d| !d.primary))
            .or_else(|| displays.iter().position(|d| d.primary)),
    }
}

pub fn has_secondary(displays: &[DisplayInfo]) -> bool {
    displays.iter().any(|d| !d.primary)
}

/// Ensure a secondary (preferably virtual) display exists for extend/external.
/// First-time users get a one-shot elevated winget install — no manual terminal.
pub fn ensure_secondary_display(mode: ShareMode) -> Result<()> {
    let list = list_displays().unwrap_or_default();
    if has_secondary(&list) {
        let _ = apply_project_mode(mode);
        return Ok(());
    }
    tracing::info!("no secondary display; provisioning virtual display driver");
    install_virtual_display_driver()?;
    // Driver arrival + topology rebuild can take a few seconds.
    for attempt in 0..24 {
        std::thread::sleep(Duration::from_millis(if attempt < 4 { 700 } else { 500 }));
        let _ = apply_project_mode(ShareMode::Extend);
        let list = list_displays().unwrap_or_default();
        if has_secondary(&list) {
            if !matches!(mode, ShareMode::Extend) {
                let _ = apply_project_mode(mode);
            }
            return Ok(());
        }
    }
    anyhow::bail!(
        "未能自动准备扩展屏。请确认已允许管理员权限，或在 Windows 显示设置中检查是否出现虚拟显示器。"
    )
}

fn install_virtual_display_driver() -> Result<()> {
    // Elevate once via UAC (same class of prompt GlideX / vendor tools show).
    // --disable-interactivity keeps winget from blocking on agreements.
    let ps = r#"
$ErrorActionPreference = 'Stop'
$winget = Get-Command winget -ErrorAction SilentlyContinue
if (-not $winget) { throw '本机没有 winget，无法自动准备扩展屏' }
$args = @(
  'install','--id=VirtualDrivers.Virtual-Display-Driver','-e',
  '--accept-package-agreements','--accept-source-agreements','--disable-interactivity'
)
$p = Start-Process -FilePath $winget.Source -ArgumentList $args -Verb RunAs -PassThru -Wait
if ($null -eq $p) { throw '用户取消了管理员确认' }
if ($p.ExitCode -ne 0 -and $p.ExitCode -ne -1978335189) {
  # -1978335189 = already installed (APPINSTALLER_CLI_ERROR_UPDATE_NOT_APPLICABLE)
  throw ("winget 退出码 " + $p.ExitCode)
}
"#;
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            ps,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("启动自动安装虚拟屏失败")?;
    if !status.success() {
        anyhow::bail!("自动准备扩展屏失败（可能取消了管理员确认）");
    }
    Ok(())
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
                let friendly = device_string_for(&name).unwrap_or_default();
                let primary = is_primary_rect(rect);
                let is_virtual = looks_virtual_display(&name, &friendly);
                out.push(DisplayInfo {
                    dxgi_index: global,
                    name,
                    friendly,
                    x: rect.left,
                    y: rect.top,
                    width,
                    height,
                    primary,
                    is_virtual,
                });
                global += 1;
                output_idx += 1;
            }
            adapter_idx += 1;
        }
        Ok(out)
    }
}

fn device_string_for(device_name: &str) -> Option<String> {
    unsafe {
        let mut wide: Vec<u16> = device_name.encode_utf16().collect();
        wide.push(0);
        let mut dd = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        // windows 0.58 takes a raw DWORD for dwFlags (not ENUM_DISPLAY_DEVICES_FLAGS).
        if EnumDisplayDevicesW(PCWSTR(wide.as_ptr()), 0, &mut dd, 0).as_bool() {
            let s = wchar_to_string(&dd.DeviceString);
            if !s.is_empty() {
                return Some(s);
            }
        }
        let mut dd2 = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        // 0x1 = EDD_GET_DEVICE_INTERFACE_NAME
        if EnumDisplayDevicesW(PCWSTR(wide.as_ptr()), 0, &mut dd2, 0x1).as_bool() {
            let s = wchar_to_string(&dd2.DeviceString);
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

struct GdiBag(Vec<DisplayInfo>);

fn list_via_gdi() -> Result<Vec<DisplayInfo>> {
    let mut bag = GdiBag(Vec::new());
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(enum_proc),
            LPARAM(&mut bag as *mut _ as isize),
        );
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
        let name = format!("Display {}", bag.0.len() + 1);
        let friendly = String::new();
        bag.0.push(DisplayInfo {
            dxgi_index: bag.0.len() as u32,
            name: name.clone(),
            friendly,
            x: r.left,
            y: r.top,
            width: (r.right - r.left).max(0) as u32,
            height: (r.bottom - r.top).max(0) as u32,
            primary: info.dwFlags & MONITORINFOF_PRIMARY == MONITORINFOF_PRIMARY,
            is_virtual: looks_virtual_display(&name, ""),
        });
    }
    BOOL(1)
}

fn is_primary_rect(rect: RECT) -> bool {
    let mut bag = GdiBag(Vec::new());
    unsafe {
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(enum_proc),
            LPARAM(&mut bag as *mut _ as isize),
        );
    }
    bag.0.iter().any(|d| {
        d.primary
            && d.x == rect.left
            && d.y == rect.top
            && d.width == (rect.right - rect.left) as u32
    })
}

fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
