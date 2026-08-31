//! Capture targets and Windows projection helpers (Win+P equivalents).
//!
//! Extend / 「仅投扩展屏」 rely on Virtual Display Driver (MttVDD). Installing the
//! package alone is not enough — we must create a monitor via the driver's named
//! pipe and set its mode to the tablet's native resolution for 1:1 encode.

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
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
const VDD_PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";
const VDD_SETTINGS: &str = r"C:\VirtualDisplayDriver\vdd_settings.xml";

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

/// Lighting never uses DisplaySwitch `/external` — that blanks the PC monitor
/// (and our host UI). Both extend modes use `/extend` and capture the secondary.
pub fn apply_project_mode(mode: ShareMode) -> Result<()> {
    let arg = match mode {
        ShareMode::Mirror => "/clone",
        ShareMode::Extend | ShareMode::External => "/extend",
    };
    let status = Command::new("DisplaySwitch.exe")
        .arg(arg)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("启动 DisplaySwitch.exe 失败")?;
    if !status.success() {
        anyhow::bail!("DisplaySwitch {arg} 返回 {status}");
    }
    std::thread::sleep(Duration::from_millis(900));
    Ok(())
}

pub fn pick_display_index(displays: &[DisplayInfo], mode: ShareMode) -> Option<usize> {
    match mode {
        ShareMode::Mirror => displays
            .iter()
            .position(|d| d.primary)
            .or(Some(0).filter(|_| !displays.is_empty())),
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

pub fn pick_virtual(displays: &[DisplayInfo]) -> Option<&DisplayInfo> {
    displays
        .iter()
        .find(|d| d.is_virtual && !d.primary)
        .or_else(|| displays.iter().find(|d| !d.primary))
}

/// Ensure a virtual secondary exists and Windows is in extend topology.
/// Does **not** blank the primary (no `/external`).
pub fn ensure_secondary_display(_mode: ShareMode) -> Result<()> {
    let list = list_displays().unwrap_or_default();
    if has_secondary(&list) {
        let _ = apply_project_mode(ShareMode::Extend);
        return Ok(());
    }

    tracing::info!("no secondary display; activating MttVDD virtual monitor");
    // Driver package may already be present but with 0 monitors — pipe first.
    if vdd_pipe_alive() {
        ensure_resolution_listed(1920, 1080, 60)?;
        vdd_set_display_count(1)?;
    } else {
        install_virtual_display_driver()?;
        // Wait for the pipe / adapter to come up.
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(500));
            if vdd_pipe_alive() {
                break;
            }
        }
        if !vdd_pipe_alive() {
            anyhow::bail!(
                "虚拟显示驱动已安装但未启动。请打开 Virtual Driver Control 点一次 Install，或重启后再试。"
            );
        }
        ensure_resolution_listed(1920, 1080, 60)?;
        vdd_set_display_count(1)?;
    }

    for attempt in 0..30 {
        std::thread::sleep(Duration::from_millis(if attempt < 6 { 700 } else { 450 }));
        let _ = apply_project_mode(ShareMode::Extend);
        let list = list_displays().unwrap_or_default();
        if has_secondary(&list) {
            return Ok(());
        }
        if attempt == 10 || attempt == 20 {
            let _ = vdd_set_display_count(1);
        }
    }
    anyhow::bail!(
        "未能创建虚拟扩展屏。请确认已允许管理员权限，并在 Windows「显示器」里能看到虚拟屏。"
    )
}

/// After tablet Hello: make the virtual monitor match tablet pixels for 1:1 capture.
pub fn configure_virtual_for_tablet(width: u32, height: u32, fps: u32) -> Result<DisplayInfo> {
    let w = (width.max(16) & !1).max(16);
    let h = (height.max(16) & !1).max(16);
    let fps = fps.clamp(30, 120);

    ensure_resolution_listed(w, h, fps)?;
    if vdd_pipe_alive() {
        // Reload so the new mode is advertised by the IDD.
        let _ = vdd_set_display_count(1);
        std::thread::sleep(Duration::from_millis(1200));
    }
    let _ = apply_project_mode(ShareMode::Extend);

    let list = list_displays().unwrap_or_default();
    let target = pick_virtual(&list)
        .cloned()
        .context("没有可用的虚拟/扩展屏")?;

    if target.width != w || target.height != h {
        if let Err(err) = set_display_mode(&target.name, w, h, fps) {
            tracing::warn!("set virtual mode {w}×{h}@{fps} failed: {err:#}");
        } else {
            std::thread::sleep(Duration::from_millis(900));
        }
    }

    let list = list_displays().unwrap_or_default();
    pick_virtual(&list)
        .cloned()
        .context("设置平板分辨率后找不到扩展屏")
}

fn vdd_pipe_alive() -> bool {
    vdd_send_command("PING").map(|r| r.to_ascii_uppercase().contains("PONG")).unwrap_or(false)
}

fn vdd_set_display_count(n: u32) -> Result<()> {
    let _ = vdd_send_command(&format!("SETDISPLAYCOUNT {n}"))
        .context("向虚拟显示驱动发送 SETDISPLAYCOUNT 失败")?;
    Ok(())
}

fn vdd_send_command(cmd: &str) -> Result<String> {
    let mut pipe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(VDD_PIPE)
        .with_context(|| format!("打开 {VDD_PIPE} 失败（驱动未运行？）"))?;
    let mut utf16: Vec<u8> = Vec::with_capacity(cmd.len() * 2);
    for unit in cmd.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    pipe.write_all(&utf16).context("写入 VDD pipe")?;
    pipe.flush().ok();
    let mut buf = vec![0u8; 4096];
    let mut out = Vec::new();
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn ensure_resolution_listed(width: u32, height: u32, fps: u32) -> Result<()> {
    let path = PathBuf::from(VDD_SETTINGS);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut xml = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        default_vdd_settings_xml()
    };
    if xml.is_empty() {
        xml = default_vdd_settings_xml();
    }

    let marker = format!("<width>{width}</width>");
    let height_tag = format!("<height>{height}</height>");
    let already = xml.contains(&marker) && xml.contains(&height_tag);
    if !already {
        let entry = format!(
            "        <resolution>\n            <width>{width}</width>\n            <height>{height}</height>\n            <refresh_rate>{fps}</refresh_rate>\n        </resolution>\n"
        );
        if let Some(idx) = xml.find("</resolutions>") {
            xml.insert_str(idx, &entry);
        } else if let Some(idx) = xml.find("</vdd_settings>") {
            let block = format!("    <resolutions>\n{entry}    </resolutions>\n");
            xml.insert_str(idx, &block);
        } else {
            xml = default_vdd_settings_xml_with(width, height, fps);
        }
    }

    // Keep at least one monitor enabled in XML.
    if !xml.contains("<count>") {
        if let Some(idx) = xml.find("<monitors>") {
            let end = idx + "<monitors>".len();
            xml.insert_str(end, "\n        <count>1</count>");
        }
    } else if xml.contains("<count>0</count>") {
        xml = xml.replace("<count>0</count>", "<count>1</count>");
    }

    std::fs::write(&path, xml).with_context(|| format!("写入 {VDD_SETTINGS} 失败"))?;
    Ok(())
}

fn default_vdd_settings_xml() -> String {
    default_vdd_settings_xml_with(1920, 1080, 60)
}

fn default_vdd_settings_xml_with(width: u32, height: u32, fps: u32) -> String {
    format!(
        r#"<?xml version='1.0' encoding='utf-8'?>
<vdd_settings>
    <monitors>
        <count>1</count>
    </monitors>
    <gpu>
        <friendlyname>default</friendlyname>
    </gpu>
    <global>
        <g_refresh_rate>60</g_refresh_rate>
        <g_refresh_rate>90</g_refresh_rate>
        <g_refresh_rate>120</g_refresh_rate>
    </global>
    <resolutions>
        <resolution>
            <width>{width}</width>
            <height>{height}</height>
            <refresh_rate>{fps}</refresh_rate>
        </resolution>
        <resolution>
            <width>1920</width>
            <height>1080</height>
            <refresh_rate>60</refresh_rate>
        </resolution>
    </resolutions>
    <logging>
        <SendLogsThroughPipe>true</SendLogsThroughPipe>
        <logging>false</logging>
        <debuglogging>false</debuglogging>
    </logging>
</vdd_settings>
"#
    )
}

fn set_display_mode(device_name: &str, width: u32, height: u32, fps: u32) -> Result<()> {
    // DEVMODEW layout differs across windows-rs versions; use a tiny elevated-free
    // PowerShell P/Invoke so we stay compatible with windows 0.58.
    let device = device_name.replace('\'', "''");
    let ps = format!(
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct DEVMODE {{
  [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
  public short dmSpecVersion; public short dmDriverVersion; public short dmSize; public short dmDriverExtra;
  public int dmFields; public int dmPositionX; public int dmPositionY; public int dmDisplayOrientation;
  public int dmDisplayFixedOutput; public short dmColor; public short dmDuplex; public short dmYResolution;
  public short dmTTOption; public short dmCollate;
  [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
  public short dmLogPixels; public int dmBitsPerPel; public int dmPelsWidth; public int dmPelsHeight;
  public int dmDisplayFlags; public int dmDisplayFrequency; public int dmICMMethod; public int dmICMIntent;
  public int dmMediaType; public int dmDitherType; public int dmReserved1; public int dmReserved2;
  public int dmPanningWidth; public int dmPanningHeight;
}}
public static class Disp {{
  public const int ENUM_CURRENT_SETTINGS = -1;
  public const int CDS_UPDATEREGISTRY = 0x01;
  public const int CDS_NORESET = 0x10000000;
  public const int DM_PELSWIDTH = 0x80000;
  public const int DM_PELSHEIGHT = 0x100000;
  public const int DM_DISPLAYFREQUENCY = 0x400000;
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern bool EnumDisplaySettings(string deviceName, int modeNum, ref DEVMODE devMode);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int ChangeDisplaySettingsEx(string lpszDeviceName, ref DEVMODE lpDevMode, IntPtr hwnd, int dwflags, IntPtr lParam);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int ChangeDisplaySettingsEx(string lpszDeviceName, IntPtr lpDevMode, IntPtr hwnd, int dwflags, IntPtr lParam);
}}
"@
$dm = New-Object DEVMODE
$dm.dmSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type][DEVMODE])
if (-not [Disp]::EnumDisplaySettings('{device}', [Disp]::ENUM_CURRENT_SETTINGS, [ref]$dm)) {{ throw 'EnumDisplaySettings failed' }}
$dm.dmPelsWidth = {width}
$dm.dmPelsHeight = {height}
$dm.dmDisplayFrequency = {fps}
$dm.dmFields = $dm.dmFields -bor [Disp]::DM_PELSWIDTH -bor [Disp]::DM_PELSHEIGHT -bor [Disp]::DM_DISPLAYFREQUENCY
$r = [Disp]::ChangeDisplaySettingsEx('{device}', [ref]$dm, [IntPtr]::Zero, ([Disp]::CDS_UPDATEREGISTRY -bor [Disp]::CDS_NORESET), [IntPtr]::Zero)
if ($r -ne 0) {{
  $dm.dmFields = [Disp]::DM_PELSWIDTH -bor [Disp]::DM_PELSHEIGHT
  $r = [Disp]::ChangeDisplaySettingsEx('{device}', [ref]$dm, [IntPtr]::Zero, ([Disp]::CDS_UPDATEREGISTRY -bor [Disp]::CDS_NORESET), [IntPtr]::Zero)
  if ($r -ne 0) {{ throw "ChangeDisplaySettingsEx stage failed: $r" }}
}}
$r2 = [Disp]::ChangeDisplaySettingsEx([NullString]::Value, [IntPtr]::Zero, [IntPtr]::Zero, 0, [IntPtr]::Zero)
if ($r2 -ne 0) {{ throw "ChangeDisplaySettingsEx apply failed: $r2" }}
"#
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("设置虚拟屏分辨率失败")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("设置虚拟屏分辨率失败: {err}");
    }
    Ok(())
}

fn install_virtual_display_driver() -> Result<()> {
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
  throw ("winget 退出码 " + $p.ExitCode)
}
"#;
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps])
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
