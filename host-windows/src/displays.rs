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
use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::PCWSTR;

use lighting_host::view::{looks_virtual_display, ShareMode};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const VDD_PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";
/// Official MttVDD default when registry `VDDPATH` is absent.
const VDD_SETTINGS_DEFAULT: &str = r"C:\VirtualDisplayDriver\vdd_settings.xml";
const VDD_REG_KEY: &str = r"HKLM:\SOFTWARE\MikeTheTech\VirtualDisplayDriver";

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

/// Apply Win+P topology. External (tablet-only) still uses `/extend` here so the
/// Lighting window stays on a visible monitor until the tablet Hello arrives;
/// [`apply_tablet_only_output`] blanks the PC panel afterwards.
pub fn apply_project_mode(mode: ShareMode) -> Result<()> {
    let arg = match mode {
        ShareMode::Mirror => "/clone",
        ShareMode::Extend | ShareMode::External => "/extend",
    };
    display_switch(arg)
}

/// Win+P “仅第二屏幕”: laptop panel off, desktop lives on the virtual display.
pub fn apply_tablet_only_output() -> Result<()> {
    display_switch("/external")
}

/// Bring the PC panel back after a tablet-only session.
pub fn restore_pc_monitor() -> Result<()> {
    display_switch("/extend")
}

fn display_switch(arg: &str) -> Result<()> {
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
    let items: Vec<(bool, bool)> = displays
        .iter()
        .map(|d| (d.primary, d.is_virtual))
        .collect();
    lighting_host::view::pick_share_target_index(&items, mode)
}

pub fn has_secondary(displays: &[DisplayInfo]) -> bool {
    displays.iter().any(|d| !d.primary)
}

pub fn has_virtual_display(displays: &[DisplayInfo]) -> bool {
    displays.iter().any(|d| d.is_virtual)
}

pub fn pick_virtual(displays: &[DisplayInfo]) -> Option<&DisplayInfo> {
    displays
        .iter()
        .find(|d| d.is_virtual)
        .or_else(|| displays.iter().find(|d| !d.primary))
}

/// Ensure a virtual secondary exists and Windows is in extend topology.
///
/// Option B (preferred): LightingIdd — our IddCx UMDF (`Root\LightingIdd`).
/// Enabling the device makes one monitor arrive (GlideX-class lifecycle).
/// Legacy fallback: bundled MttVDD + named-pipe SETDISPLAYCOUNT.
pub fn ensure_secondary_display(_mode: ShareMode) -> Result<()> {
    let list = list_displays().unwrap_or_default();
    if has_secondary(&list) {
        let _ = apply_project_mode(ShareMode::Extend);
        return Ok(());
    }

    if find_idd_bundle().is_some() {
        tracing::info!("no secondary; provisioning LightingIdd (IddCx Option B)");
        match ensure_via_lighting_idd() {
            Ok(()) => return Ok(()),
            Err(err) => tracing::warn!("LightingIdd failed ({err:#}); falling back to MttVDD"),
        }
    } else {
        tracing::info!("LightingIdd bundle absent; using legacy MttVDD path");
    }

    tracing::info!("provisioning bundled MttVDD (legacy fallback)");
    let _ = prepare_vdd_settings(1920, 1080, 60);

    if !vdd_pipe_alive() {
        let _ = run_vdd_provision(VddProvisionMode::EnableOnly);
        wait_for_vdd_pipe(10);
    }
    if !vdd_pipe_alive() {
        run_vdd_provision(VddProvisionMode::Full)?;
        wait_for_vdd_pipe(24);
    }
    if !vdd_pipe_alive() {
        let _ = install_virtual_display_driver_winget();
        let _ = run_vdd_provision(VddProvisionMode::Full);
        wait_for_vdd_pipe(16);
    }
    if !vdd_pipe_alive() {
        anyhow::bail!("VDD_PIPE_DOWN");
    }

    let _ = run_vdd_provision(VddProvisionMode::EnableOnly);
    wait_for_vdd_pipe(16);
    if vdd_pipe_alive() {
        let _ = vdd_set_display_count(1);
        wait_for_pipe_reload(20);
    }

    if poll_secondary(40) {
        return Ok(());
    }

    let _ = prepare_vdd_settings(1920, 1080, 60);
    let _ = run_vdd_provision(VddProvisionMode::EnableOnly);
    wait_for_vdd_pipe(16);
    let _ = vdd_set_display_count(1);
    wait_for_pipe_reload(24);
    if poll_secondary(30) {
        return Ok(());
    }
    anyhow::bail!("VDD_NO_MONITOR");
}

fn ensure_via_lighting_idd() -> Result<()> {
    let _ = run_idd_provision(IddProvisionMode::EnableOnly);
    if poll_secondary(12) {
        return Ok(());
    }
    run_idd_provision(IddProvisionMode::Full)?;
    if poll_secondary(40) {
        return Ok(());
    }
    let _ = run_idd_provision(IddProvisionMode::EnableOnly);
    if poll_secondary(20) {
        return Ok(());
    }
    anyhow::bail!("IDD_NO_MONITOR")
}

#[derive(Clone, Copy)]
enum IddProvisionMode {
    Full,
    EnableOnly,
}

fn find_idd_bundle() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("idd"));
            candidates.push(dir.join("resources").join("idd"));
        }
    }
    if let Ok(runtime) = std::env::var("LIGHTING_RUNTIME_DIR") {
        candidates.push(PathBuf::from(runtime).join("idd"));
    }
    if let Ok(resources) = std::env::var("LIGHTING_RESOURCES_DIR") {
        candidates.push(PathBuf::from(resources).join("idd"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("host-ui")
            .join("resources")
            .join("idd"),
    );
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("scripts")
            .join("idd"),
    );
    candidates
        .into_iter()
        .find(|p| p.join("LightingIdd.inf").is_file())
}

fn run_idd_provision(mode: IddProvisionMode) -> Result<()> {
    let bundle = find_idd_bundle().context("IDD_BUNDLE_MISSING")?;
    let script = bundle.join("provision.ps1");
    let script = if script.is_file() {
        script
    } else {
        let alt = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("scripts")
            .join("idd")
            .join("provision.ps1");
        if !alt.is_file() {
            anyhow::bail!("IDD_SCRIPT_MISSING");
        }
        alt
    };
    let mode_s = match mode {
        IddProvisionMode::Full => "Full",
        IddProvisionMode::EnableOnly => "EnableOnly",
    };
    run_elevated_provision_script(&script, &bundle, mode_s)
}

fn run_elevated_provision_script(
    script: &std::path::Path,
    bundle: &std::path::Path,
    mode: &str,
) -> Result<()> {
    let result_file = std::env::temp_dir().join(format!(
        "lighting-idd-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let bundle_s = bundle.to_string_lossy().replace('\'', "''");
    let script_s = script.to_string_lossy().replace('\'', "''");
    let result_s = result_file.to_string_lossy().replace('\'', "''");
    let ps = format!(
        r#"$ErrorActionPreference='Stop'
$rf='{result_s}'
$bd='{bundle_s}'
$scr='{script_s}'
$m='{mode}'
$p=Start-Process powershell.exe -Verb RunAs -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$scr,'-BundleDir',$bd,'-ResultFile',$rf,'-Mode',$m) -PassThru -Wait -WindowStyle Hidden
if($null -eq $p){{Set-Content -Path $rf -Value 'FAIL|UAC_DENIED' -Encoding ASCII -NoNewline; exit 1}}
if(-not (Test-Path $rf)){{Set-Content -Path $rf -Value 'FAIL|UAC_CANCELLED' -Encoding ASCII -NoNewline; exit 1}}
exit 0"#
    );
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("IddCx provision launcher failed")?;
    if !status.success() {
        anyhow::bail!("IDD_LAUNCHER_FAILED");
    }
    let raw = std::fs::read_to_string(&result_file).unwrap_or_default();
    let _ = std::fs::remove_file(&result_file);
    let line = raw.trim();
    if line.starts_with("OK|") {
        tracing::info!("idd provision ok: {line}");
        return Ok(());
    }
    if line.starts_with("FAIL|") {
        anyhow::bail!("{}", line.trim_start_matches("FAIL|"));
    }
    anyhow::bail!("IDD_UNKNOWN_RESULT")
}


fn poll_secondary(attempts: u32) -> bool {
    for attempt in 0..attempts {
        std::thread::sleep(Duration::from_millis(if attempt < 8 { 800 } else { 500 }));
        let _ = apply_project_mode(ShareMode::Extend);
        let list = list_displays().unwrap_or_default();
        if has_secondary(&list) {
            return true;
        }
    }
    false
}

/// SETDISPLAYCOUNT triggers an adapter reload; pipe drops then returns.
fn wait_for_pipe_reload(attempts: u32) {
    // Brief window for disconnect.
    for _ in 0..6 {
        if !vdd_pipe_alive() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    wait_for_vdd_pipe(attempts);
}

#[derive(Clone, Copy)]
enum VddProvisionMode {
    Full,
    EnableOnly,
}

fn find_vdd_bundle() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("vdd"));
            candidates.push(dir.join("resources").join("vdd"));
        }
    }
    if let Ok(runtime) = std::env::var("LIGHTING_RUNTIME_DIR") {
        candidates.push(PathBuf::from(runtime).join("vdd"));
    }
    if let Ok(resources) = std::env::var("LIGHTING_RESOURCES_DIR") {
        candidates.push(PathBuf::from(resources).join("vdd"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("host-ui")
            .join("resources")
            .join("vdd"),
    );
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("scripts")
            .join("vdd"),
    );
    candidates.into_iter().find(|p| p.join("MttVDD.inf").is_file())
}

/// Run bundled provision.ps1 elevated; never surfaces raw PowerShell stderr (GBK garble).
fn run_vdd_provision(mode: VddProvisionMode) -> Result<()> {
    let bundle = find_vdd_bundle().context("VDD_BUNDLE_MISSING")?;
    let script = bundle.join("provision.ps1");
    if !script.is_file() {
        anyhow::bail!("VDD_SCRIPT_MISSING");
    }
    let result_file = std::env::temp_dir().join(format!(
        "lighting-vdd-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let mode_s = match mode {
        VddProvisionMode::Full => "Full",
        VddProvisionMode::EnableOnly => "EnableOnly",
    };
    let bundle_s = bundle.to_string_lossy().replace('\'', "''");
    let script_s = script.to_string_lossy().replace('\'', "''");
    let result_s = result_file.to_string_lossy().replace('\'', "''");
    let ps = format!(
        r#"$ErrorActionPreference='Stop'
$rf='{result_s}'
$bd='{bundle_s}'
$scr='{script_s}'
$m='{mode_s}'
$p=Start-Process powershell.exe -Verb RunAs -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$scr,'-BundleDir',$bd,'-ResultFile',$rf,'-Mode',$m) -PassThru -Wait -WindowStyle Hidden
if($null -eq $p){{Set-Content -Path $rf -Value 'FAIL|UAC_DENIED' -Encoding ASCII -NoNewline; exit 1}}
if(-not (Test-Path $rf)){{Set-Content -Path $rf -Value 'FAIL|UAC_CANCELLED' -Encoding ASCII -NoNewline; exit 1}}
exit 0"#
    );
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .context("VDD provision launcher failed")?;
    if !status.success() {
        anyhow::bail!("VDD_LAUNCHER_FAILED");
    }
    let raw = std::fs::read_to_string(&result_file).unwrap_or_default();
    let _ = std::fs::remove_file(&result_file);
    let line = raw.trim();
    if line.starts_with("OK|") {
        tracing::info!("vdd provision ok: {line}");
        return Ok(());
    }
    if line.starts_with("FAIL|") {
        let code = line.trim_start_matches("FAIL|");
        anyhow::bail!("{}", code);
    }
    anyhow::bail!("VDD_UNKNOWN_RESULT")
}

fn wait_for_vdd_pipe(attempts: u32) {
    for _ in 0..attempts {
        if vdd_pipe_alive() {
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// After tablet Hello: make the virtual monitor match tablet pixels for 1:1 capture.
pub fn configure_virtual_for_tablet(width: u32, height: u32, fps: u32) -> Result<DisplayInfo> {
    let w = (width.max(16) & !1).max(16);
    let h = (height.max(16) & !1).max(16);
    let fps = fps.clamp(30, 120);

    let _ = prepare_vdd_settings(w, h, fps);
    if vdd_pipe_alive() {
        // Soft-restart is preferred; SETDISPLAYCOUNT reload is a fallback nudge.
        let _ = run_vdd_provision(VddProvisionMode::EnableOnly);
        wait_for_vdd_pipe(10);
        let _ = vdd_set_display_count(1);
        wait_for_pipe_reload(12);
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

/// Resolve MttVDD settings directory (registry `VDDPATH`, else official default).
fn resolve_vdd_dir() -> PathBuf {
    let ps = format!(
        r#"$ErrorActionPreference='SilentlyContinue'; $p=(Get-ItemProperty -Path '{VDD_REG_KEY}' -Name VDDPATH).VDDPATH; if($p){{$p}}"#
    );
    if let Ok(out) = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    PathBuf::from(r"C:\VirtualDisplayDriver")
}

/// Write official-schema `vdd_settings.xml` with count=1 and best-effort registry VDDPATH.
/// Matches GlideX-style “driver already knows it should expose one monitor”.
fn prepare_vdd_settings(width: u32, height: u32, fps: u32) -> Result<()> {
    let dir = resolve_vdd_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("vdd_settings.xml");
    let fallback = PathBuf::from(VDD_SETTINGS_DEFAULT);

    let mut xml = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else if fallback.exists() {
        std::fs::read_to_string(&fallback).unwrap_or_default()
    } else {
        String::new()
    };
    if xml.is_empty() || !xml.contains("<vdd_settings>") {
        xml = default_vdd_settings_xml_with(width, height, fps);
    }

    let marker = format!("<width>{width}</width>");
    let height_tag = format!("<height>{height}</height>");
    if !(xml.contains(&marker) && xml.contains(&height_tag)) {
        let entry = format!(
            "        <resolution>\n            <width>{width}</width>\n            <height>{height}</height>\n            <refresh_rate>{fps}</refresh_rate>\n        </resolution>\n"
        );
        if let Some(idx) = xml.find("</resolutions>") {
            xml.insert_str(idx, &entry);
        } else {
            xml = default_vdd_settings_xml_with(width, height, fps);
        }
    }

    if xml.contains("<count>0</count>") {
        xml = xml.replace("<count>0</count>", "<count>1</count>");
    } else if !xml.contains("<count>") {
        if let Some(idx) = xml.find("<monitors>") {
            let end = idx + "<monitors>".len();
            xml.insert_str(end, "\n        <count>1</count>");
        }
    }

    std::fs::write(&path, &xml).with_context(|| format!("写入 {} 失败", path.display()))?;
    if path != fallback {
        if let Some(parent) = fallback.parent() {
            let _ = std::fs::create_dir_all(parent);
            let _ = std::fs::write(&fallback, &xml);
        }
    }

    // Best-effort registry (elevated provision.ps1 also writes this).
    let dir_s = dir.to_string_lossy().replace('\'', "''");
    let reg_ps = format!(
        r#"$ErrorActionPreference='SilentlyContinue'; New-Item -Path '{VDD_REG_KEY}' -Force | Out-Null; Set-ItemProperty -Path '{VDD_REG_KEY}' -Name VDDPATH -Value '{dir_s}' -Type String"#
    );
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &reg_ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    Ok(())
}

fn default_vdd_settings_xml_with(width: u32, height: u32, fps: u32) -> String {
    // Schema matches VirtualDrivers/Virtual-Display-Driver 25.7.23 sample.
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
        <g_refresh_rate>144</g_refresh_rate>
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
        <resolution>
            <width>2560</width>
            <height>1440</height>
            <refresh_rate>60</refresh_rate>
        </resolution>
        <resolution>
            <width>3840</width>
            <height>2160</height>
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

/// Snapshot used to restore the PC monitor after a follow-tablet session.
#[derive(Debug, Clone)]
pub struct ModeRestore {
    pub device: String,
    pub mode: DisplayMode,
}

/// RAII: put the monitor back when the cast client session ends.
pub struct ModeRestoreGuard(pub Option<ModeRestore>);

impl Drop for ModeRestoreGuard {
    fn drop(&mut self) {
        if let Some(restore) = self.0.take() {
            if let Err(err) = set_display_mode(
                &restore.device,
                restore.mode.width,
                restore.mode.height,
                restore.mode.fps.max(30),
            ) {
                tracing::warn!(
                    "restore display mode {}×{}@{} on {} failed: {err:#}",
                    restore.mode.width,
                    restore.mode.height,
                    restore.mode.fps,
                    restore.device
                );
            } else {
                tracing::info!(
                    "restored display {} to {}×{}@{}",
                    restore.device,
                    restore.mode.width,
                    restore.mode.height,
                    restore.mode.fps
                );
            }
        }
    }
}

/// Holds `SetThreadExecutionState` on a dedicated thread so idle timeout cannot
/// lock the session (DXGI cannot capture the secure desktop).
pub struct KeepAwakeGuard {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl KeepAwakeGuard {
    pub fn acquire() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("lighting-keep-awake".into())
            .spawn(move || {
                unsafe {
                    let _ = SetThreadExecutionState(
                        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED,
                    );
                }
                let _ = rx.recv();
                unsafe {
                    let _ = SetThreadExecutionState(ES_CONTINUOUS);
                }
            })
            .ok();
        Self {
            stop: Some(tx),
            thread,
        }
    }
}

impl Drop for KeepAwakeGuard {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Best-effort: lid close → do nothing while sharing, then restore.
pub struct LidCloseGuard {
    prev: Option<(u32, u32)>,
}

impl LidCloseGuard {
    pub fn apply() -> Self {
        let prev = query_lid_actions();
        apply_lid_action(0);
        if prev.is_none() {
            tracing::info!("lid close set to do-nothing (previous value unknown)");
        }
        Self { prev }
    }
}

impl Drop for LidCloseGuard {
    fn drop(&mut self) {
        if let Some((ac, dc)) = self.prev.take() {
            apply_lid_ac_dc(ac, dc);
        }
        // If we never parsed the previous values, leave "do nothing" in place —
        // that is the setting the user needs for bed use.
    }
}

fn powercfg(args: &[&str]) -> Option<std::process::Output> {
    Command::new("powercfg.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()
}

fn query_lid_actions() -> Option<(u32, u32)> {
    let out = powercfg(&["/q", "SCHEME_CURRENT", "SUB_BUTTONS"])?;
    let text = String::from_utf8_lossy(&out.stdout);
    lighting_host::session_policy::parse_lid_current_indices(&text)
}

fn apply_lid_action(index: u32) {
    apply_lid_ac_dc(index, index);
}

fn apply_lid_ac_dc(ac: u32, dc: u32) {
    let ac_s = ac.to_string();
    let dc_s = dc.to_string();
    let _ = powercfg(&[
        "/SETACVALUEINDEX",
        "SCHEME_CURRENT",
        "SUB_BUTTONS",
        "LIDACTION",
        &ac_s,
    ]);
    let _ = powercfg(&[
        "/SETDCVALUEINDEX",
        "SCHEME_CURRENT",
        "SUB_BUTTONS",
        "LIDACTION",
        &dc_s,
    ]);
    let _ = powercfg(&["/SETACTIVE", "SCHEME_CURRENT"]);
}

/// Enumerate modes the adapter exposes for `device_name` (e.g. `\\.\DISPLAY1`).
pub fn list_display_modes(device_name: &str) -> Result<Vec<DisplayMode>> {
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
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern bool EnumDisplaySettings(string deviceName, int modeNum, ref DEVMODE devMode);
}}
"@
$dm = New-Object DEVMODE
$dm.dmSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type][DEVMODE])
$i = 0
while ([Disp]::EnumDisplaySettings('{device}', $i, [ref]$dm)) {{
  if ($dm.dmPelsWidth -gt 0 -and $dm.dmPelsHeight -gt 0) {{
    Write-Output ("{{0}}x{{1}}@{{2}}" -f $dm.dmPelsWidth, $dm.dmPelsHeight, [Math]::Max(30, $dm.dmDisplayFrequency))
  }}
  $i++
  if ($i -gt 512) {{ break }}
}}
"#
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("枚举显示器模式失败")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("枚举显示器模式失败: {err}");
    }
    let mut modes = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 2560x1440@60
        let (wh, fps) = match line.split_once('@') {
            Some(pair) => pair,
            None => continue,
        };
        let (w, h) = match wh.split_once('x') {
            Some(pair) => pair,
            None => continue,
        };
        let (Ok(w), Ok(h), Ok(fps)) = (w.parse::<u32>(), h.parse::<u32>(), fps.parse::<u32>()) else {
            continue;
        };
        modes.push(DisplayMode {
            width: w,
            height: h,
            fps: fps.max(30),
        });
    }
    modes.sort_by_key(|m| (m.width, m.height, m.fps));
    modes.dedup();
    Ok(modes)
}

/// Pick the closest hardware mode to the oriented tablet panel.
pub fn pick_closest_mode(
    modes: &[DisplayMode],
    target_w: u32,
    target_h: u32,
    prefer_fps: u32,
    native_w: u32,
    native_h: u32,
) -> Option<DisplayMode> {
    let tuples: Vec<(u32, u32, u32)> = modes
        .iter()
        .map(|m| (m.width, m.height, m.fps))
        .collect();
    lighting_host::session_policy::pick_closest_display_mode(
        &tuples,
        target_w,
        target_h,
        prefer_fps,
        native_w,
        native_h,
    )
    .map(|(w, h, fps)| DisplayMode {
        width: w,
        height: h,
        fps,
    })
}

/// Read the monitor's live mode, including refresh rate.
pub fn current_display_mode(device_name: &str) -> Result<DisplayMode> {
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
public static class DispCur {{
  public const int ENUM_CURRENT_SETTINGS = -1;
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern bool EnumDisplaySettings(string deviceName, int modeNum, ref DEVMODE devMode);
}}
"@
$dm = New-Object DEVMODE
$dm.dmSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type][DEVMODE])
if (-not [DispCur]::EnumDisplaySettings('{device}', [DispCur]::ENUM_CURRENT_SETTINGS, [ref]$dm)) {{ throw 'EnumDisplaySettings failed' }}
Write-Output ("{{0}}x{{1}}@{{2}}" -f $dm.dmPelsWidth, $dm.dmPelsHeight, $dm.dmDisplayFrequency)
"#
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("读取当前显示模式失败")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("读取当前显示模式失败: {err}");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.trim();
    let (wh, fps) = line.split_once('@').context("解析当前显示模式失败")?;
    let (w, h) = wh.split_once('x').context("解析当前显示模式失败")?;
    Ok(DisplayMode {
        width: w.trim().parse().context("解析宽度失败")?,
        height: h.trim().parse().context("解析高度失败")?,
        fps: fps.trim().parse::<u32>().unwrap_or(60).max(30),
    })
}

/// Follow-tablet: temporarily switch the captured monitor toward the tablet panel.
/// Never puts a 16:9 laptop onto 1920×1200 just because the tablet is 16:10 —
/// that scaled timing is what made 1080p smooth and 1200p stutter.
pub fn apply_follow_tablet_mode(
    device_name: &str,
    current_size: DisplayMode,
    tablet_w: u32,
    tablet_h: u32,
    min_fps: u32,
) -> Result<(DisplayMode, ModeRestore)> {
    let target_w = tablet_w.max(16);
    let target_h = tablet_h.max(16);

    let current = current_display_mode(device_name).unwrap_or(current_size);
    let min_fps = min_fps.max(30).min(current.fps.max(30));
    let modes = list_display_modes(device_name).unwrap_or_default();
    let tuples: Vec<(u32, u32, u32)> = modes
        .iter()
        .map(|m| (m.width, m.height, m.fps))
        .collect();
    let (native_w, native_h) = lighting_host::session_policy::native_panel_mode(&tuples)
        .map(|(w, h, _)| (w, h))
        .unwrap_or((current.width, current.height));

    let chosen = pick_closest_mode(
        &modes,
        target_w,
        target_h,
        min_fps,
        native_w,
        native_h,
    )
    .context("显示器没有可用的分辨率列表")?;

    if !lighting_host::session_policy::should_switch_desktop_mode(
        (current.width, current.height, current.fps),
        (chosen.width, chosen.height, chosen.fps),
        target_w,
        target_h,
        native_w,
        native_h,
    ) {
        anyhow::bail!(
            "电脑保持 {}×{}@{}Hz（原生比例 {}×{}，不切 16:10 的 {}×{} 以免卡顿）",
            current.width,
            current.height,
            current.fps,
            native_w,
            native_h,
            target_w,
            target_h
        );
    }

    set_display_mode(device_name, chosen.width, chosen.height, chosen.fps)?;
    std::thread::sleep(Duration::from_millis(900));
    Ok((
        chosen,
        ModeRestore {
            device: device_name.to_string(),
            mode: current,
        },
    ))
}

pub fn set_display_mode(device_name: &str, width: u32, height: u32, fps: u32) -> Result<()> {
    // DEVMODEW layout differs across windows-rs versions; use a tiny elevated-free
    // PowerShell P/Invoke so we stay compatible with windows 0.58.
    // Use a *temporary* mode change (no CDS_UPDATEREGISTRY) so casting does not
    // permanently rewrite the user's preferred resolution.
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
  public const int DM_PELSWIDTH = 0x80000;
  public const int DM_PELSHEIGHT = 0x100000;
  public const int DM_DISPLAYFREQUENCY = 0x400000;
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern bool EnumDisplaySettings(string deviceName, int modeNum, ref DEVMODE devMode);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int ChangeDisplaySettingsEx(string lpszDeviceName, ref DEVMODE lpDevMode, IntPtr hwnd, int dwflags, IntPtr lParam);
}}
"@
$dm = New-Object DEVMODE
$dm.dmSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type][DEVMODE])
if (-not [Disp]::EnumDisplaySettings('{device}', [Disp]::ENUM_CURRENT_SETTINGS, [ref]$dm)) {{ throw 'EnumDisplaySettings failed' }}
$dm.dmPelsWidth = {width}
$dm.dmPelsHeight = {height}
$dm.dmDisplayFrequency = {fps}
$dm.dmFields = [Disp]::DM_PELSWIDTH -bor [Disp]::DM_PELSHEIGHT -bor [Disp]::DM_DISPLAYFREQUENCY
$r = [Disp]::ChangeDisplaySettingsEx('{device}', [ref]$dm, [IntPtr]::Zero, 0, [IntPtr]::Zero)
if ($r -ne 0) {{
  $dm.dmFields = [Disp]::DM_PELSWIDTH -bor [Disp]::DM_PELSHEIGHT
  $r = [Disp]::ChangeDisplaySettingsEx('{device}', [ref]$dm, [IntPtr]::Zero, 0, [IntPtr]::Zero)
  if ($r -ne 0) {{ throw "ChangeDisplaySettingsEx failed: $r" }}
}}
"#
    );
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("设置显示器分辨率失败")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("设置显示器分辨率失败: {err}");
    }
    Ok(())
}

fn install_virtual_display_driver_winget() -> Result<()> {
    let ps = r#"
$ErrorActionPreference = 'SilentlyContinue'
$winget = Get-Command winget -ErrorAction SilentlyContinue
if (-not $winget) { exit 0 }
$p = Start-Process -FilePath $winget.Source -ArgumentList @('install','--id=VirtualDrivers.Virtual-Display-Driver','-e','--accept-package-agreements','--accept-source-agreements','--disable-interactivity') -Verb RunAs -PassThru -Wait -WindowStyle Hidden
if ($null -eq $p) { exit 0 }
exit 0
"#;
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
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
