//! Capture targets and Windows projection helpers (Win+P equivalents).
//!
//! Virtual monitors are provisioned by the bundled, signed MttVDD driver.
//! A healthy PnP device is necessary but not sufficient: only an active desktop
//! monitor makes extension ready. The optional control pipe is not a health gate.

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use windows::Win32::Foundation::{BOOL, CloseHandle, HANDLE, LPARAM, RECT, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Devices::Display::{
    SetDisplayConfig, SDC_APPLY, SDC_TOPOLOGY_CLONE, SDC_TOPOLOGY_EXTEND,
    SDC_TOPOLOGY_EXTERNAL, SDC_TOPOLOGY_INTERNAL, SET_DISPLAY_CONFIG_FLAGS,
};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplayMonitors, EnumDisplaySettingsW, GetMonitorInfoW,
    DEVMODEW, DISPLAY_DEVICEW, ENUM_CURRENT_SETTINGS, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows::Win32::System::Power::{
    SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcessToken, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::{MONITORINFOF_PRIMARY, SW_SHOWNORMAL};
use windows::core::PCWSTR;

use lighting_host::capture_graph::DxgiCapture;
use lighting_host::view::{looks_virtual_display, ShareMode};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const VDD_PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";
/// Official MttVDD default when registry `VDDPATH` is absent.
const VDD_SETTINGS_DEFAULT: &str = r"C:\VirtualDisplayDriver\vdd_settings.xml";
const VDD_REG_KEY: &str = r"HKLM:\SOFTWARE\MikeTheTech\VirtualDisplayDriver";
const PROVISION_TIMEOUT: Duration = Duration::from_secs(180);

/// True when this process already has an elevated token (run as admin).
pub fn process_is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(std::ptr::addr_of_mut!(elev).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        ok.is_ok() && elev.TokenIsElevated != 0
    }
}

#[derive(Debug, Clone)]
pub struct DisplayInfo {
    /// Absent for GDI-only virtual monitors; never synthesize a DXGI index.
    pub dxgi: Option<DxgiCapture>,
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
            &self.name
        } else {
            &self.friendly
        };
        format!(
            "{kind} {w}×{h} · {title}",
            w = self.width,
            h = self.height,
        )
    }
}

pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    // GDI describes the active desktop, including indirect displays DXGI omits.
    // DXGI only enriches that list with an adapter-local duplication identity.
    let mut displays = list_via_gdi()?;
    match list_via_dxgi() {
        Ok(outputs) => {
            for display in &mut displays {
                display.dxgi = outputs
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&display.name))
                    .map(|(_, capture)| *capture);
            }
        }
        Err(err) => tracing::warn!("DXGI enum failed ({err:#}); using selected GDI regions"),
    }
    Ok(displays)
}

/// Apply Win+P topology. External (tablet-only) still uses `/extend` here so the
/// Lighting window stays on a visible monitor until the tablet Hello arrives;
/// [`apply_tablet_only_output`] blanks the PC panel afterwards.
pub fn apply_project_mode(mode: ShareMode) -> Result<()> {
    let topology = match mode {
        ShareMode::Mirror => SDC_TOPOLOGY_CLONE,
        ShareMode::Extend | ShareMode::External => SDC_TOPOLOGY_EXTEND,
    };
    apply_topology(topology)
}

/// Win+P “仅第二屏幕”: laptop panel off, desktop lives on the virtual display.
pub fn apply_tablet_only_output() -> Result<()> {
    apply_topology(SDC_TOPOLOGY_EXTERNAL)
}

/// Last-resort CCD restore when we do not have a primary snapshot. Prefer
/// [`restore_desktop`]: SDC_TOPOLOGY_EXTEND replays the polluted extend slot.
pub fn restore_pc_monitor() -> Result<()> {
    let code = unsafe {
        SetDisplayConfig(
            None,
            None,
            SDC_APPLY | SDC_TOPOLOGY_INTERNAL | SDC_TOPOLOGY_CLONE | SDC_TOPOLOGY_EXTEND | SDC_TOPOLOGY_EXTERNAL,
        )
    };
    if code != 0 {
        anyhow::bail!("DISPLAY_TOPOLOGY_FAILED:{code}");
    }
    std::thread::sleep(Duration::from_millis(400));
    Ok(())
}

fn apply_topology(topology: SET_DISPLAY_CONFIG_FLAGS) -> Result<()> {
    let code = unsafe { SetDisplayConfig(None, None, SDC_APPLY | topology) };
    if code != 0 {
        anyhow::bail!("DISPLAY_TOPOLOGY_FAILED:{code}");
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
    pick_virtual_excluding(displays, None)
}

pub fn pick_virtual_excluding<'a>(
    displays: &'a [DisplayInfo],
    primary_device: Option<&str>,
) -> Option<&'a DisplayInfo> {
    let not_primary = |d: &&DisplayInfo| {
        primary_device
            .map(|p| lighting_host::session_policy::is_safe_virtual_target(&d.name, p))
            .unwrap_or(true)
    };
    displays
        .iter()
        .filter(not_primary)
        .find(|d| d.is_virtual)
        .or_else(|| displays.iter().filter(not_primary).find(|d| !d.primary))
}

/// Ensure a real secondary desktop exists; never equate driver-store staging or
/// a control-pipe heartbeat with a working second screen.
pub fn ensure_secondary_display(mode: ShareMode) -> Result<()> {
    ensure_secondary_display_with_progress(mode, |_| {}, None)
}

pub fn ensure_secondary_display_with_progress(
    mode: ShareMode,
    mut progress: impl FnMut(&str),
    preserve: Option<&PrimarySnapshot>,
) -> Result<()> {
    let reassert = |progress: &mut dyn FnMut(&str)| {
        if let Some(snap) = preserve {
            progress("正在确认主屏原始刷新率…");
            if let Err(err) = reassert_primary(snap) {
                tracing::warn!("reassert primary after extend: {err:#}");
            }
        }
    };

    progress("正在启用 Windows 扩展桌面…");
    if has_secondary(&list_displays().unwrap_or_default()) {
        reassert(&mut progress);
        if poll_share_target(2, &mut progress, mode, preserve) {
            return Ok(());
        }
    } else if let Err(err) = apply_project_mode(ShareMode::Extend) {
        // No second target may exist yet. Provision it before requiring a
        // successful topology change; privilege does not create a target.
        tracing::info!("initial extend topology unavailable: {err:#}");
    }
    reassert(&mut progress);
    if poll_share_target(4, &mut progress, mode, preserve) {
        return Ok(());
    }

    progress(lighting_host::share_flow::virtual_driver_install_copy(
        process_is_elevated(),
    ));
    // Full also repairs an existing unhealthy root device. INF-only LightingIdd
    // bundles are not installable drivers and must not precede this signed path.
    run_vdd_provision()?;
    if !has_secondary(&list_displays().unwrap_or_default()) {
        if let Err(err) = apply_project_mode(ShareMode::Extend) {
            tracing::warn!("extend after driver install: {err:#}");
        }
    }
    reassert(&mut progress);
    if poll_share_target(20, &mut progress, mode, preserve) {
        progress("虚拟屏已就绪");
        return Ok(());
    }

    // A user may have configured zero virtual monitors. Request one only when
    // none appeared; do not reduce a working multi-monitor configuration.
    if vdd_pipe_alive() {
        progress("正在请求虚拟显示驱动创建屏幕…");
        vdd_set_display_count(1)?;
        reassert(&mut progress);
        if poll_share_target(20, &mut progress, mode, preserve) {
            return Ok(());
        }
    }
    anyhow::bail!("VDD_NO_MONITOR:{}", mode.as_wire())
}

/// Installer launcher.
///
/// `lighting-host --ipc-only` has no window. `Start-Process -Verb RunAs
/// -WindowStyle Hidden` from that process either never shows UAC, or (when
/// already admin) hides the installer until it appears hung. Run in-process
/// when elevated; otherwise ShellExecute "runas" with a visible PowerShell.
fn run_provision_script(
    script: &std::path::Path,
    bundle: &std::path::Path,
    mode: &str,
) -> Result<()> {
    let result_file = std::env::temp_dir().join(format!(
        "lighting-drv-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_file(&result_file);

    let elevated = process_is_elevated();
    tracing::info!(elevated, mode, "launching virtual display provision");
    if elevated {
        run_provision_already_admin(script, bundle, mode, &result_file)?;
    } else {
        run_provision_with_uac(script, bundle, mode, &result_file)?;
    }

    let raw = std::fs::read_to_string(&result_file).unwrap_or_default();
    let _ = std::fs::remove_file(&result_file);
    let line = raw.trim();
    if line.starts_with("OK|") {
        tracing::info!("driver provision ok: {line}");
        return Ok(());
    }
    if line.starts_with("FAIL|") {
        anyhow::bail!("{}", line.trim_start_matches("FAIL|"));
    }
    anyhow::bail!("DRIVER_UNKNOWN_RESULT")
}

fn run_provision_already_admin(
    script: &std::path::Path,
    bundle: &std::path::Path,
    mode: &str,
    result_file: &std::path::Path,
) -> Result<()> {
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.to_string_lossy(),
            "-BundleDir",
            &bundle.to_string_lossy(),
            "-ResultFile",
            &result_file.to_string_lossy(),
            "-Mode",
            mode,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("DRIVER_LAUNCHER_FAILED")?;
    wait_child_with_timeout(&mut child, PROVISION_TIMEOUT)
}

fn wait_child_with_timeout(child: &mut std::process::Child, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return Ok(()),
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("INSTALL_TIMEOUT");
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(150)),
            Err(err) => anyhow::bail!("DRIVER_LAUNCHER_FAILED:{err}"),
        }
    }
}

fn run_provision_with_uac(
    script: &std::path::Path,
    bundle: &std::path::Path,
    mode: &str,
    result_file: &std::path::Path,
) -> Result<()> {
    let params = format!(
        "-NoProfile -ExecutionPolicy Bypass -File \"{}\" -BundleDir \"{}\" -ResultFile \"{}\" -Mode {}",
        script.display(),
        bundle.display(),
        result_file.display(),
        mode
    );
    let file = windows::core::HSTRING::from("powershell.exe");
    let args = windows::core::HSTRING::from(params.as_str());
    let dir = windows::core::HSTRING::from(bundle.to_string_lossy().as_ref());
    let verb = windows::core::w!("runas");
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: verb,
            lpFile: windows::core::PCWSTR(file.as_ptr()),
            lpParameters: windows::core::PCWSTR(args.as_ptr()),
            lpDirectory: windows::core::PCWSTR(dir.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        ShellExecuteExW(&mut info).context("UAC_DENIED")?;
        if info.hProcess.is_invalid() {
            anyhow::bail!("UAC_CANCELLED");
        }
        let wait = WaitForSingleObject(info.hProcess, PROVISION_TIMEOUT.as_millis() as u32);
        // WAIT_TIMEOUT is WIN32_ERROR; WaitForSingleObject returns WAIT_EVENT.
        if wait.0 == WAIT_TIMEOUT.0 {
            let _ = TerminateProcess(info.hProcess, 1);
            let _ = CloseHandle(info.hProcess);
            anyhow::bail!("UAC_TIMEOUT");
        }
        if wait != WAIT_OBJECT_0 {
            let _ = CloseHandle(info.hProcess);
            anyhow::bail!("UAC_CANCELLED");
        }
        let _ = CloseHandle(info.hProcess);
    }
    if !result_file.is_file() {
        anyhow::bail!("UAC_CANCELLED");
    }
    Ok(())
}


fn poll_share_target(
    attempts: u32,
    progress: &mut impl FnMut(&str),
    mode: ShareMode,
    preserve: Option<&PrimarySnapshot>,
) -> bool {
    for attempt in 0..attempts {
        progress(&format!(
            "正在等待虚拟屏出现（{}/{}）…",
            attempt + 1,
            attempts
        ));
        std::thread::sleep(Duration::from_millis(if attempt < 8 { 800 } else { 500 }));
        if let Some(snap) = preserve {
            let _ = reassert_primary(snap);
        }
        let list = list_displays().unwrap_or_default();
        if has_secondary(&list) || (mode == ShareMode::External && has_virtual_display(&list)) {
            return true;
        }
    }
    false
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
fn run_vdd_provision() -> Result<()> {
    let bundle = find_vdd_bundle().context("VDD_BUNDLE_MISSING")?;
    let script = bundle.join("provision.ps1");
    if !script.is_file() {
        anyhow::bail!("VDD_SCRIPT_MISSING");
    }
    run_provision_script(&script, &bundle, "Full")
}

pub fn configure_virtual_for_tablet(
    width: u32,
    height: u32,
    fps: u32,
    preserve: Option<&PrimarySnapshot>,
) -> Result<DisplayInfo> {
    let w = (width.max(16) & !1).max(16);
    let h = (height.max(16) & !1).max(16);
    let _fps = fps.clamp(30, 120);
    let primary_name = preserve.map(|p| p.device.as_str());

    // Never reload the IddCx adapter here: InitAdapter makes Windows promote
    // the virtual panel and drops Desktop Duplication (black host screen).
    let list = list_displays()?;
    let target = pick_virtual_excluding(&list, primary_name)
        .cloned()
        .context("没有可用的虚拟/扩展屏")?;
    anyhow::ensure!(
        primary_name
            .map(|p| lighting_host::session_policy::is_safe_virtual_target(&target.name, p))
            .unwrap_or(true),
        "拒绝改写主屏 {}",
        target.name
    );

    if target.width != w || target.height != h {
        // Size only — setting refresh on the virtual path retimes the whole
        // desktop and is what followed the tablet Hz onto the laptop panel.
        if let Err(err) = change_display_mode(&target.name, w, h, None, None, false) {
            tracing::warn!("set virtual mode {w}×{h} failed: {err:#}");
        } else {
            std::thread::sleep(Duration::from_millis(400));
        }
    }
    if let Some(snap) = preserve {
        if let Err(err) = reassert_primary(snap) {
            tracing::warn!("reassert primary after virtual mode: {err:#}");
        }
    }

    let list = list_displays()?;
    list.into_iter()
        .find(|d| d.name.eq_ignore_ascii_case(&target.name))
        .or_else(|| pick_virtual_excluding(&list, primary_name).cloned())
        .context("设置平板分辨率后找不到原扩展屏")
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
    // The upstream pipe accepts UTF-16 commands and emits UTF-8 logs/PONG,
    // then disconnects. Drain asynchronously with a deadline: a stalled UMDF
    // driver must not leave Start Sharing blocked forever.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut pipe = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(VDD_PIPE)
                .context("打开虚拟显示驱动控制管道失败")?;
            let bytes: Vec<u8> = cmd.encode_utf16().flat_map(u16::to_le_bytes).collect();
            pipe.write_all(&bytes).await.context("写入 VDD pipe")?;
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                match pipe.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        out.extend_from_slice(&buf[..n]);
                        if out.len() > 65536 {
                            anyhow::bail!("VDD_PIPE_RESPONSE_TOO_LARGE");
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => break,
                    Err(err) => return Err(err).context("读取 VDD pipe"),
                }
            }
            Ok(String::from_utf8_lossy(&out).into_owned())
        })
        .await
        .context("VDD_PIPE_TIMEOUT")?
    })
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

/// Physical panel as it was before Lighting touched CCD / DEVMODE.
#[derive(Debug, Clone)]
pub struct PrimarySnapshot {
    pub device: String,
    pub mode: DisplayMode,
    pub x: i32,
    pub y: i32,
}

pub fn snapshot_primary() -> Result<PrimarySnapshot> {
    let list = list_displays()?;
    let primary = list
        .iter()
        .find(|d| d.primary)
        .cloned()
        .context("没有主显示器")?;
    let mode = current_display_mode(&primary.name).unwrap_or(DisplayMode {
        width: primary.width,
        height: primary.height,
        // 0 = unknown; restore must not invent 60 Hz on a high-refresh panel.
        fps: 0,
    });
    Ok(PrimarySnapshot {
        device: primary.name,
        mode,
        x: primary.x,
        y: primary.y,
    })
}

/// Put the laptop/host panel back as primary with its original timing.
/// Temporary (no CDS_UPDATEREGISTRY) so we do not rewrite the user's profile.
pub fn restore_primary(snap: &PrimarySnapshot) -> Result<()> {
    change_display_mode(
        &snap.device,
        snap.mode.width,
        snap.mode.height,
        (snap.mode.fps >= 30).then_some(snap.mode.fps),
        Some((snap.x, snap.y)),
        true,
    )
}

/// Live path: restore Hz/primary only when they actually drifted.
/// Repeated CDS_SET_PRIMARY is what blanks the laptop and kills capture.
pub fn reassert_primary(snap: &PrimarySnapshot) -> Result<()> {
    let list = list_displays().unwrap_or_default();
    let current = list
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(&snap.device));
    let fps = current
        .and_then(|d| current_display_mode(&d.name).ok())
        .map(|m| m.fps)
        .unwrap_or(0);
    let action = lighting_host::session_policy::primary_restore_action(
        current.map(|d| d.name.as_str()),
        current.map(|d| d.primary).unwrap_or(false),
        current.map(|d| d.width).unwrap_or(0),
        current.map(|d| d.height).unwrap_or(0),
        fps,
        &snap.device,
        snap.mode.width,
        snap.mode.height,
        snap.mode.fps,
    );
    match action {
        lighting_host::session_policy::PrimaryRestoreAction::Skip => Ok(()),
        lighting_host::session_policy::PrimaryRestoreAction::TimingOnly => change_display_mode(
            &snap.device,
            snap.mode.width,
            snap.mode.height,
            (snap.mode.fps >= 30).then_some(snap.mode.fps),
            Some((snap.x, snap.y)),
            false,
        ),
        lighting_host::session_policy::PrimaryRestoreAction::SetPrimary => restore_primary(snap),
    }
}

pub fn restore_desktop(snap: &PrimarySnapshot) -> Result<()> {
    if let Err(err) = restore_primary(snap) {
        tracing::warn!("restore_primary failed: {err:#}");
    }
    let list = list_displays().unwrap_or_default();
    let present = list
        .iter()
        .any(|d| d.name.eq_ignore_ascii_case(&snap.device));
    if !present {
        let _ = restore_pc_monitor();
        restore_primary(snap)?;
    } else if let Err(err) = restore_primary(snap) {
        tracing::warn!("second restore_primary failed: {err:#}");
    }
    Ok(())
}

/// RAII: always restore the host panel, including after a mid-share interrupt.
pub struct DesktopRestoreGuard {
    pub primary: Option<PrimarySnapshot>,
}

impl DesktopRestoreGuard {
    pub fn capture() -> Self {
        match snapshot_primary() {
            Ok(primary) => {
                tracing::info!(
                    "captured primary {} {}×{}@{}Hz origin=({},{})",
                    primary.device,
                    primary.mode.width,
                    primary.mode.height,
                    primary.mode.fps,
                    primary.x,
                    primary.y
                );
                Self {
                    primary: Some(primary),
                }
            }
            Err(err) => {
                tracing::warn!("could not snapshot primary display: {err:#}");
                Self { primary: None }
            }
        }
    }
}

impl Drop for DesktopRestoreGuard {
    fn drop(&mut self) {
        if let Some(primary) = self.primary.take() {
            if let Err(err) = restore_desktop(&primary) {
                tracing::warn!("restore desktop after share failed: {err:#}");
            } else {
                tracing::info!(
                    "restored primary {} to {}×{}@{}Hz",
                    primary.device,
                    primary.mode.width,
                    primary.mode.height,
                    primary.mode.fps
                );
            }
        }
    }
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

fn current_display_mode_gdi(device_name: &str) -> Result<DisplayMode> {
    let mut wide: Vec<u16> = device_name.encode_utf16().collect();
    wide.push(0);
    let mut dm = DEVMODEW::default();
    dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    unsafe {
        anyhow::ensure!(
            EnumDisplaySettingsW(PCWSTR(wide.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm).as_bool(),
            "EnumDisplaySettingsW failed"
        );
    }
    anyhow::ensure!(dm.dmPelsWidth > 0 && dm.dmPelsHeight > 0, "empty current mode");
    Ok(DisplayMode {
        width: dm.dmPelsWidth,
        height: dm.dmPelsHeight,
        fps: dm.dmDisplayFrequency,
    })
}

/// Read the monitor's live mode, including refresh rate.
pub fn current_display_mode(device_name: &str) -> Result<DisplayMode> {
    match current_display_mode_gdi(device_name) {
        Ok(mode) => return Ok(mode),
        Err(err) => tracing::warn!("native EnumDisplaySettings failed ({err:#}); trying PowerShell"),
    }
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
        fps: fps.trim().parse::<u32>().unwrap_or(0),
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
    change_display_mode(device_name, width, height, Some(fps.max(30)), None, false)
}

fn change_display_mode(
    device_name: &str,
    width: u32,
    height: u32,
    fps: Option<u32>,
    position: Option<(i32, i32)>,
    set_primary: bool,
) -> Result<()> {
    // Temporary (no CDS_UPDATEREGISTRY): do not rewrite the user's preferred mode.
    let device = device_name.replace('\'', "''");
    let freq_line = if let Some(fps) = fps {
        format!("$dm.dmDisplayFrequency = {fps}\n$dm.dmFields = $dm.dmFields -bor [Disp]::DM_DISPLAYFREQUENCY")
    } else {
        String::new()
    };
    let pos_line = if let Some((x, y)) = position {
        format!("$dm.dmPositionX = {x}\n$dm.dmPositionY = {y}\n$dm.dmFields = $dm.dmFields -bor [Disp]::DM_POSITION")
    } else {
        String::new()
    };
    let flags = if set_primary { "0x10000010" } else { "0" }; // CDS_NORESET|CDS_SET_PRIMARY or none
    let apply = if set_primary {
        "$r2 = [Disp]::ChangeDisplaySettingsExPtr($null, [IntPtr]::Zero, [IntPtr]::Zero, 0, [IntPtr]::Zero); if ($r2 -ne 0) { throw \"ChangeDisplaySettingsEx apply failed: $r2\" }"
    } else {
        ""
    };
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
  public const int DM_POSITION = 0x20;
  public const int DM_PELSWIDTH = 0x80000;
  public const int DM_PELSHEIGHT = 0x100000;
  public const int DM_DISPLAYFREQUENCY = 0x400000;
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern bool EnumDisplaySettings(string deviceName, int modeNum, ref DEVMODE devMode);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int ChangeDisplaySettingsEx(string lpszDeviceName, ref DEVMODE lpDevMode, IntPtr hwnd, int dwflags, IntPtr lParam);
  [DllImport("user32.dll", CharSet = CharSet.Unicode, EntryPoint = "ChangeDisplaySettingsEx")]
  public static extern int ChangeDisplaySettingsExPtr(string lpszDeviceName, IntPtr lpDevMode, IntPtr hwnd, int dwflags, IntPtr lParam);
}}
"@
$dm = New-Object DEVMODE
$dm.dmSize = [System.Runtime.InteropServices.Marshal]::SizeOf([type][DEVMODE])
if (-not [Disp]::EnumDisplaySettings('{device}', [Disp]::ENUM_CURRENT_SETTINGS, [ref]$dm)) {{ throw 'EnumDisplaySettings failed' }}
$dm.dmPelsWidth = {width}
$dm.dmPelsHeight = {height}
$dm.dmFields = [Disp]::DM_PELSWIDTH -bor [Disp]::DM_PELSHEIGHT
{freq}
{pos}
$r = [Disp]::ChangeDisplaySettingsEx('{device}', [ref]$dm, [IntPtr]::Zero, {flags}, [IntPtr]::Zero)
if ($r -ne 0) {{ throw "ChangeDisplaySettingsEx failed: $r" }}
{apply}
"#,
        device = device,
        width = width,
        height = height,
        freq = freq_line,
        pos = pos_line,
        flags = flags,
        apply = apply,
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


fn list_via_dxgi() -> Result<Vec<(String, DxgiCapture)>> {
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().context("CreateDXGIFactory1")?;
        let mut out = Vec::new();
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
                if !desc.AttachedToDesktop.as_bool() || width == 0 || height == 0 {
                    output_idx += 1;
                    continue;
                }
                let name = wchar_to_string(&desc.DeviceName);
                out.push((
                    name,
                    DxgiCapture {
                        adapter_index: adapter_idx,
                        output_index: output_idx,
                    },
                ));
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
        let mut monitor = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        let mut friendly = String::new();
        if EnumDisplayDevicesW(PCWSTR(wide.as_ptr()), 0, &mut monitor, 0).as_bool() {
            friendly = wchar_to_string(&monitor.DeviceString);
        }
        // A virtual adapter's monitor can have a generic PnP name. Include the
        // owning adapter description so virtual-display classification survives.
        let mut index = 0;
        loop {
            let mut adapter = DISPLAY_DEVICEW {
                cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if !EnumDisplayDevicesW(PCWSTR::null(), index, &mut adapter, 0).as_bool() {
                break;
            }
            if wchar_to_string(&adapter.DeviceName).eq_ignore_ascii_case(device_name) {
                let description = wchar_to_string(&adapter.DeviceString);
                if !description.is_empty() && description != friendly {
                    if !friendly.is_empty() {
                        friendly.push_str(" · ");
                    }
                    friendly.push_str(&description);
                }
                break;
            }
            index += 1;
        }
        (!friendly.is_empty()).then_some(friendly)
    }
}

struct GdiBag(Vec<DisplayInfo>);

fn list_via_gdi() -> Result<Vec<DisplayInfo>> {
    let mut bag = GdiBag(Vec::new());
    unsafe {
        anyhow::ensure!(
            EnumDisplayMonitors(
                HDC::default(),
                None,
                Some(enum_proc),
                LPARAM(&mut bag as *mut _ as isize),
            )
            .as_bool(),
            "EnumDisplayMonitors/GetMonitorInfoW failed"
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
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
        return BOOL(0);
    }
    let r = info.monitorInfo.rcMonitor;
    let width = (r.right - r.left).max(0) as u32;
    let height = (r.bottom - r.top).max(0) as u32;
    if width == 0 || height == 0 {
        return BOOL(1);
    }
    let name = wchar_to_string(&info.szDevice);
    let friendly = device_string_for(&name).unwrap_or_default();
    let is_virtual = looks_virtual_display(&name, &friendly);
    bag.0.push(DisplayInfo {
        dxgi: None,
        name,
        friendly,
        x: r.left,
        y: r.top,
        width,
        height,
        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY == MONITORINFOF_PRIMARY,
        is_virtual,
    });
    BOOL(1)
}

fn wchar_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
