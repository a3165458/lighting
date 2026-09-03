//! Share-start policy and on-screen activity that can be simulated on Linux.
//!
//! Windows DXGI / DisplaySwitch are not available in this environment; the
//! decision table and the activity timeline are what we unit-test before a
//! Windows build.

use crate::view::ShareMode;

/// What to do after `ensure_secondary_display` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualPrepareOutcome {
    Ready,
    FallbackMirror { reason: String },
    Abort { reason: String },
}

/// Tablet-only must never silently become mirror — that is the bug users hit.
pub fn decide_after_virtual_prepare(
    mode: ShareMode,
    ensure_ok: bool,
    ensure_err: &str,
    has_secondary: bool,
    has_virtual: bool,
) -> VirtualPrepareOutcome {
    let present = has_secondary || has_virtual;
    if ensure_ok && present {
        return VirtualPrepareOutcome::Ready;
    }
    let reason = if !ensure_ok && !ensure_err.trim().is_empty() {
        ensure_err.trim().to_string()
    } else {
        "虚拟屏未能创建（驱动未就绪）".into()
    };
    if mode.blanks_pc_monitor() || !mode.allows_mirror_fallback() {
        VirtualPrepareOutcome::Abort { reason }
    } else {
        VirtualPrepareOutcome::FallbackMirror { reason }
    }
}

pub fn tablet_only_abort_message(reason: &str) -> String {
    format!(
        "仅平板需要虚拟屏，没有改用镜像。{reason} 若已用管理员运行仍失败，请完全退出 Lighting 后重试；未提权时请看是否有蓝底「用户账户控制」。"
    )
}

/// Progress / error copy when the portable shipped INF without LightingIdd.dll.
pub fn idd_bundle_incomplete_copy() -> &'static str {
    "安装包缺少虚拟屏驱动文件"
}

/// True only when both INF and DLL are present. INF-only must not launch provision.ps1.
pub fn should_attempt_idd_install(inf_present: bool, dll_present: bool) -> bool {
    inf_present && dll_present
}

pub fn is_idd_bundle_incomplete(raw: &str) -> bool {
    let u = raw.to_uppercase();
    u.contains("BUNDLE_DLL_MISSING") || u.contains("BUNDLE_INF_MISSING")
}

/// Copy shown while installing the virtual display driver.
pub fn virtual_driver_install_copy(already_admin: bool) -> &'static str {
    if already_admin {
        "已是管理员，正在直接安装虚拟显示驱动（不会再弹出确认窗口）。若弹出 360/杀毒软件请选允许…"
    } else {
        "需要安装驱动。请在蓝底「用户账户控制」窗口点「是」；若弹出 360/杀毒软件也请选允许。"
    }
}

/// First-line notice when the user clicks 开始共享.
pub fn share_start_notice(mode: ShareMode, already_admin: bool) -> String {
    if mode.blanks_pc_monitor() {
        if already_admin {
            "正在启用虚拟屏。已是管理员，不会再弹出确认窗口。若弹出 360/杀毒软件请选允许。".into()
        } else {
            "正在启用虚拟屏。当前步骤会显示在上方；请留意管理员确认窗口。".into()
        }
    } else if mode.uses_virtual_display() {
        if already_admin {
            "正在启用独立第二屏。已是管理员，不会再弹出确认窗口。若弹出 360/杀毒软件请选允许。".into()
        } else {
            "正在启用独立第二屏（虚拟显示器）…".into()
        }
    } else {
        "正在开始镜像投屏…".into()
    }
}

/// Hide `last_error` when usbHint / activity already show the same abort copy.
pub fn should_show_separate_last_error(last_error: &str, already_shown: &[&str]) -> bool {
    let last = last_error.trim();
    if last.is_empty() {
        return false;
    }
    !already_shown.iter().any(|shown| {
        let shown = shown.trim();
        !shown.is_empty() && (shown == last || shown.contains(last) || last.contains(shown))
    })
}

/// Outcome of an unelevated provision launch (UAC / ShellExecute).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisionUacError {
    Cancelled,
    Timeout,
    NoResult,
    AccessDenied,
    ShellExecuteFailed,
    WaitFailed,
    HiddenHost,
}

impl ProvisionUacError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "UAC_CANCELLED",
            Self::Timeout => "UAC_TIMEOUT",
            Self::NoResult => "UAC_NO_RESULT",
            Self::AccessDenied => "ACCESS_DENIED",
            Self::ShellExecuteFailed => "SHELLEXECUTE_FAILED",
            Self::WaitFailed => "UAC_WAIT_FAILED",
            Self::HiddenHost => "UAC_HIDDEN_HOST",
        }
    }
}

/// Classify a UAC / ShellExecute launch. A missing result file is never "cancelled".
pub fn classify_provision_uac(
    shellexecute_ok: bool,
    win32_error: u32,
    process_valid: bool,
    timed_out: bool,
    wait_signaled: bool,
    result_file_present: bool,
) -> Result<(), ProvisionUacError> {
    const ERROR_ACCESS_DENIED: u32 = 5;
    const ERROR_CANCELLED: u32 = 1223;
    if !shellexecute_ok {
        return Err(match win32_error {
            ERROR_CANCELLED => ProvisionUacError::Cancelled,
            ERROR_ACCESS_DENIED => ProvisionUacError::AccessDenied,
            _ => ProvisionUacError::ShellExecuteFailed,
        });
    }
    if !process_valid {
        return Err(ProvisionUacError::ShellExecuteFailed);
    }
    if timed_out {
        return Err(ProvisionUacError::Timeout);
    }
    if !wait_signaled {
        return Err(ProvisionUacError::WaitFailed);
    }
    if !result_file_present {
        return Err(ProvisionUacError::NoResult);
    }
    Ok(())
}

/// After an already-elevated provision child finishes. Missing file / killed
/// child / timeout / unknown payload are never `UAC_CANCELLED`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisionFinishError {
    Interrupted,
    NoResult,
    UnknownResult,
    Timeout,
}

impl ProvisionFinishError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Interrupted => "INSTALL_INTERRUPTED",
            Self::NoResult => "DRIVER_NO_RESULT",
            Self::UnknownResult => "DRIVER_UNKNOWN_RESULT",
            Self::Timeout => "INSTALL_TIMEOUT",
        }
    }
}

pub fn classify_provision_finish(
    timed_out: bool,
    exit_success: bool,
    result_file_present: bool,
    result_line: &str,
) -> Result<(), ProvisionFinishError> {
    if timed_out {
        return Err(ProvisionFinishError::Timeout);
    }
    if !result_file_present {
        // 360 often kills the child before Write-Result. Missing file is
        // an interrupt, even if wait() saw a zero exit.
        let _ = exit_success;
        return Err(ProvisionFinishError::Interrupted);
    }
    let line = result_line.trim();
    if line.starts_with("OK|") || line.starts_with("FAIL|") {
        return Ok(());
    }
    Err(ProvisionFinishError::Interrupted)
}

/// Hardware IDs of the adapters Lighting actually installs.
pub const MTT_VDD_HWID: &str = r"Root\MttVDD";
pub const LIGHTING_IDD_HWID: &str = r"Root\LightingIdd";

/// `pnputil /add-driver` 0 or 259 only means the INF is in the driver store.
pub fn driver_store_staged(exit_code: i32) -> bool {
    exit_code == 0 || exit_code == 259
}

/// A root-enumerated software device still needs nefcon/devcon `install`
/// unless an instance for the intended HWID already exists.
pub fn should_create_root_device_node(device_present: bool) -> bool {
    !device_present
}

/// `pnputil /enable-device` exit 0 is not proof the instance exists.
pub fn enable_device_exit_proves_presence(_exit_code: i32) -> bool {
    false
}

/// One PnP node as reported by `Get-PnpDevice` or `pnputil /enum-devices`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PnpDeviceMatch {
    pub instance_id: String,
    pub hardware_ids: Vec<String>,
    pub friendly_name: String,
    pub class_name: String,
    pub status: String,
    pub problem_code: Option<u32>,
}

fn hwid_token(hwid: &str) -> &str {
    hwid.rsplit(['\\', '/']).next().filter(|s| !s.is_empty()).unwrap_or(hwid)
}

fn normalize_id_blob(s: &str) -> String {
    s.to_ascii_uppercase()
        .replace('/', r"\")
        .replace('-', "_")
}

fn looks_like_glidex(blob: &str) -> bool {
    blob.to_ascii_lowercase().contains("glidex")
}

/// True when `blob` (instance / hardware IDs) contains `token` as a path segment.
pub fn blob_has_hwid_token(blob: &str, token: &str) -> bool {
    let blob = normalize_id_blob(blob);
    let token = token.trim().to_ascii_uppercase();
    if token.is_empty() {
        return false;
    }
    blob.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part == token)
}

fn id_blob(dev: &PnpDeviceMatch) -> String {
    let mut parts = vec![dev.instance_id.as_str()];
    for id in &dev.hardware_ids {
        parts.push(id.as_str());
    }
    parts.join("|")
}

/// Match `Root\MttVDD` / `Root\LightingIdd` (or that driver's real HWID).
/// GlideX and a generic “Virtual Display” name are not enough.
pub fn is_intended_virtual_adapter(dev: &PnpDeviceMatch, intended_hwid: &str) -> bool {
    let ids = id_blob(dev);
    let names = format!("{}|{}", ids, dev.friendly_name);
    if looks_like_glidex(&names) {
        return false;
    }
    let token = hwid_token(intended_hwid);
    blob_has_hwid_token(&ids, token)
        || ids.to_ascii_uppercase().contains(&intended_hwid.to_ascii_uppercase())
}

/// Started Display / System / SoftwareDevice (or IddCx) node for the HWID.
/// Problem or Unknown-class nodes that never started are not ready.
pub fn is_ready_virtual_adapter(dev: &PnpDeviceMatch, intended_hwid: &str) -> bool {
    if !is_intended_virtual_adapter(dev, intended_hwid) {
        return false;
    }
    let class = dev.class_name.trim().to_ascii_lowercase();
    if class == "unknown" {
        return false;
    }
    let status = dev.status.trim().to_ascii_lowercase();
    if status.contains("problem") || status == "error" || status == "disabled" {
        return false;
    }
    if dev.problem_code.unwrap_or(0) != 0 {
        return false;
    }
    true
}

/// `OK|READY` / `OK|ENABLED` is not success unless the detail names our HWID.
pub fn provision_ok_names_intended_device(line: &str) -> bool {
    let line = line.trim();
    let Some(rest) = line.strip_prefix("OK|") else {
        return false;
    };
    let detail = rest
        .split_once(':')
        .map(|(_, d)| d)
        .unwrap_or("")
        .trim();
    if detail.is_empty() || detail.eq_ignore_ascii_case("unknown") {
        return false;
    }
    if looks_like_glidex(detail) {
        return false;
    }
    blob_has_hwid_token(detail, "MTTVDD")
        || blob_has_hwid_token(detail, "LIGHTINGIDD")
        || blob_has_hwid_token(detail, "IDDSAMPLEDRIVER")
}

/// QA's false OK: `OK|READY` / `OK|ENABLED` with no intended instance.
pub fn provision_claimed_ok_without_device(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("OK|") && !provision_ok_names_intended_device(line)
}

/// Decision table for already-elevated Idd / MttVDD Full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeProvisionFinish {
    OkReady(String),
    OkEnabled(String),
    Fail(&'static str),
}

pub fn finish_native_full(
    staged: bool,
    enable_exit_ok: bool,
    device: Option<&PnpDeviceMatch>,
    intended_hwid: &str,
) -> NativeProvisionFinish {
    let _ = enable_exit_ok;
    if let Some(dev) = device {
        if is_ready_virtual_adapter(dev, intended_hwid) {
            return NativeProvisionFinish::OkReady(dev.instance_id.clone());
        }
    }
    if staged {
        NativeProvisionFinish::Fail("DEVICE_STILL_MISSING")
    } else {
        NativeProvisionFinish::Fail("DRIVER_INSTALL_FAILED")
    }
}

pub fn finish_native_enable_only(
    enable_exit_ok: bool,
    device: Option<&PnpDeviceMatch>,
    intended_hwid: &str,
) -> NativeProvisionFinish {
    let _ = enable_exit_ok;
    if let Some(dev) = device {
        if is_ready_virtual_adapter(dev, intended_hwid) {
            return NativeProvisionFinish::OkEnabled(dev.instance_id.clone());
        }
    }
    NativeProvisionFinish::Fail("DEVICE_NOT_FOUND")
}

fn pnputil_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let line = line.trim();
    if line.len() < key.len() {
        return None;
    }
    if !line[..key.len()].eq_ignore_ascii_case(key) {
        return None;
    }
    Some(line[key.len()..].trim())
}

/// Parse `pnputil /enum-devices` text. “No devices were found” is empty.
pub fn parse_pnputil_enum_devices(stdout: &str) -> Vec<PnpDeviceMatch> {
    if stdout.to_ascii_lowercase().contains("no devices were found") {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = PnpDeviceMatch::default();
    let flush = |cur: &mut PnpDeviceMatch, out: &mut Vec<PnpDeviceMatch>| {
        if !cur.instance_id.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    for raw in stdout.lines() {
        let line = raw.trim();
        if let Some(v) = pnputil_field(line, "Instance ID:") {
            flush(&mut cur, &mut out);
            cur.instance_id = v.to_string();
        } else if let Some(v) = pnputil_field(line, "Device Description:") {
            cur.friendly_name = v.to_string();
        } else if let Some(v) = pnputil_field(line, "Class Name:") {
            cur.class_name = v.to_string();
        } else if let Some(v) = pnputil_field(line, "Status:") {
            cur.status = v.to_string();
        } else if let Some(v) = pnputil_field(line, "Problem Code:") {
            let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
            cur.problem_code = digits.parse().ok();
        } else if let Some(v) = pnputil_field(line, "Hardware ID:") {
            if !v.is_empty() {
                cur.hardware_ids.push(v.to_string());
            }
        } else if !line.is_empty()
            && raw.starts_with([' ', '\t'])
            && (line.to_ascii_uppercase().starts_with("ROOT\\")
                || line.to_ascii_uppercase().contains("MTTVDD")
                || line.to_ascii_uppercase().contains("LIGHTINGIDD"))
        {
            cur.hardware_ids.push(line.to_string());
        }
    }
    flush(&mut cur, &mut out);
    out
}

pub fn first_ready_intended<'a>(
    devices: &'a [PnpDeviceMatch],
    intended_hwid: &str,
) -> Option<&'a PnpDeviceMatch> {
    devices
        .iter()
        .find(|d| is_ready_virtual_adapter(d, intended_hwid))
}

pub fn first_intended_adapter<'a>(
    devices: &'a [PnpDeviceMatch],
    intended_hwid: &str,
) -> Option<&'a PnpDeviceMatch> {
    devices
        .iter()
        .find(|d| is_intended_virtual_adapter(d, intended_hwid))
}

/// LightingIdd was interrupted by AV / missing result — do not hide it behind
/// MttVDD `DEVICE_NOT_FOUND` / `VDD_NO_MONITOR`.
pub fn should_surface_provision_interrupt(raw: &str) -> bool {
    let u = raw.to_uppercase();
    u.contains("INSTALL_INTERRUPTED")
        || u.contains("DRIVER_NO_RESULT")
        || u.contains("DRIVER_UNKNOWN_RESULT")
        || u.contains("DRIVER_LAUNCHER_FAILED")
        || u.contains("INSTALL_TIMEOUT")
}

fn looks_like_vdd_device_missing(raw: &str) -> bool {
    let u = raw.to_uppercase();
    u.contains("DEVICE_NOT_FOUND")
        || u.contains("DEVICE_STILL_MISSING")
        || u.contains("VDD_NO_MONITOR")
        || u.contains("IDD_NO_MONITOR")
}

/// Final error after LightingIdd then optional MttVDD. Interrupts win.
/// Incomplete Idd (missing DLL) must not be swallowed into 「未找到虚拟显示设备」.
pub fn choose_virtual_prepare_error(idd_err: Option<&str>, vdd_err: Option<&str>) -> String {
    if let Some(idd) = idd_err {
        if should_surface_provision_interrupt(idd) {
            return idd.trim().to_string();
        }
        if is_idd_bundle_incomplete(idd)
            && vdd_err
                .map(looks_like_vdd_device_missing)
                .unwrap_or(true)
        {
            return idd.trim().to_string();
        }
    }
    vdd_err
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| idd_err.map(str::trim).filter(|s| !s.is_empty()))
        .unwrap_or("VDD_NO_MONITOR")
        .to_string()
}

/// Drop Electron's `Error invoking remote method '…': Error:` wrapper.
pub fn strip_electron_ipc_error(raw: &str) -> &str {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("Error invoking remote method '") {
        if let Some(idx) = rest.find("': ") {
            let after = &rest[idx + 3..];
            return after
                .strip_prefix("Error: ")
                .or_else(|| after.strip_prefix("Error:"))
                .unwrap_or(after)
                .trim();
        }
    }
    raw
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityStage {
    Idle,
    PrepareVirtual,
    WaitTablet,
    SetTabletMode,
    BlankPc,
    Streaming,
    Error,
    Stopped,
}

pub fn activity_stage(phase: &str) -> ActivityStage {
    match phase {
        "" => ActivityStage::Idle,
        "错误" => ActivityStage::Error,
        "已停止" => ActivityStage::Stopped,
        "已连接" | "编码" | "回退" | "共享中" => ActivityStage::Streaming,
        "仅平板" => ActivityStage::BlankPc,
        "独立第二屏" | "适配平板" => ActivityStage::SetTabletMode,
        "准备虚拟屏" | "启用驱动" | "启动" | "启动中" => {
            ActivityStage::PrepareVirtual
        }
        "等待设备" | "监听" | "USB" | "USB 警告" | "等待" => ActivityStage::WaitTablet,
        _ => {
            if phase.contains("虚拟") {
                ActivityStage::PrepareVirtual
            } else {
                ActivityStage::WaitTablet
            }
        }
    }
}

pub fn activity_title(running: bool, phase: &str) -> String {
    if matches!(phase, "已连接" | "编码" | "回退" | "共享中") {
        return "正在投屏".into();
    }
    match activity_stage(phase) {
        ActivityStage::Idle => "未开始共享".into(),
        ActivityStage::PrepareVirtual => "正在启用虚拟屏".into(),
        ActivityStage::WaitTablet => "等待平板连接".into(),
        ActivityStage::SetTabletMode => "正在适配平板分辨率".into(),
        ActivityStage::BlankPc => "正在关闭电脑屏".into(),
        ActivityStage::Streaming => "正在投屏".into(),
        ActivityStage::Error => "未能开始共享".into(),
        ActivityStage::Stopped => {
            if running {
                "正在停止".into()
            } else {
                "已停止".into()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityStep {
    pub id: &'static str,
    pub label: &'static str,
    pub state: &'static str,
}

/// Checklist shown while a share is live. `external` adds the “关掉电脑屏” step.
pub fn activity_steps(mode: ShareMode, phase: &str, running: bool) -> Vec<ActivityStep> {
    let stage = activity_stage(phase);
    let virtual_mode = mode.uses_virtual_display();
    let mut ids: Vec<(&str, &str)> = Vec::new();
    if virtual_mode {
        ids.push(("prepare", "启用虚拟屏"));
    }
    ids.push(("wait", "等待平板连接"));
    if virtual_mode {
        ids.push(("size", "设置平板分辨率"));
    } else {
        ids.push(("size", "按平板分辨率编码"));
    }
    if mode.blanks_pc_monitor() {
        ids.push(("blank", "关闭电脑屏"));
    }
    ids.push(("stream", "开始推流"));

    let current = match stage {
        ActivityStage::PrepareVirtual => "prepare",
        ActivityStage::WaitTablet => "wait",
        ActivityStage::SetTabletMode => "size",
        ActivityStage::BlankPc => "blank",
        ActivityStage::Streaming => "stream",
        ActivityStage::Error => "prepare",
        ActivityStage::Idle | ActivityStage::Stopped => {
            if running {
                "wait"
            } else {
                ""
            }
        }
    };

    let current = if !virtual_mode && current == "prepare" {
        "wait"
    } else if !mode.blanks_pc_monitor() && current == "blank" {
        "stream"
    } else {
        current
    };

    let order: Vec<&str> = ids.iter().map(|(id, _)| *id).collect();
    let cur_idx = order.iter().position(|id| *id == current);

    ids.into_iter()
        .enumerate()
        .map(|(i, (id, label))| {
            let state = if phase == "错误" && cur_idx == Some(i) {
                "error"
            } else if !running && phase != "错误" {
                "pending"
            } else if stage == ActivityStage::Streaming && id == "stream" {
                "current"
            } else if let Some(c) = cur_idx {
                if i < c {
                    "done"
                } else if i == c {
                    "current"
                } else {
                    "pending"
                }
            } else {
                "pending"
            };
            ActivityStep { id, label, state }
        })
        .collect()
}

/// Linux-side dry run of the tablet-only session copy (no DXGI).
pub fn simulated_tablet_only_timeline() -> Vec<(&'static str, &'static str)> {
    vec![
        ("准备虚拟屏", "正在检查是否已有扩展屏…"),
        ("准备虚拟屏", "正在启用虚拟显示驱动（可能弹出管理员确认）…"),
        ("准备虚拟屏", "正在等待虚拟屏出现…"),
        ("等待设备", "请在平板上打开 Lighting 并连接"),
        ("独立第二屏", "正在把虚拟屏设为平板分辨率 1920×1200"),
        ("仅平板", "正在关闭电脑屏（Win+P 仅第二屏幕）…"),
        ("编码", "正在推流"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tablet_only_never_falls_back_to_mirror() {
        let out =
            decide_after_virtual_prepare(ShareMode::External, false, "UAC_CANCELLED", false, false);
        assert!(
            matches!(out, VirtualPrepareOutcome::Abort { .. }),
            "{out:?}"
        );
        let out = decide_after_virtual_prepare(ShareMode::External, true, "", false, false);
        assert!(matches!(out, VirtualPrepareOutcome::Abort { .. }));
    }

    #[test]
    fn extend_may_fall_back_to_mirror() {
        let out =
            decide_after_virtual_prepare(ShareMode::Extend, false, "VDD_PIPE_DOWN", false, false);
        assert!(matches!(out, VirtualPrepareOutcome::FallbackMirror { .. }));
    }

    #[test]
    fn virtual_ready_when_only_virtual_is_primary() {
        let out = decide_after_virtual_prepare(ShareMode::External, true, "", false, true);
        assert_eq!(out, VirtualPrepareOutcome::Ready);
    }

    #[test]
    fn secondary_without_heuristic_is_enough() {
        let out = decide_after_virtual_prepare(ShareMode::External, true, "", true, false);
        assert_eq!(out, VirtualPrepareOutcome::Ready);
    }

    #[test]
    fn abort_copy_says_we_did_not_switch_to_mirror() {
        let msg = tablet_only_abort_message("已取消管理员授权。");
        assert!(msg.contains("没有改用镜像"));
        assert!(virtual_driver_install_copy(true).contains("不会再弹出"));
        assert!(virtual_driver_install_copy(false).contains("用户账户控制"));
    }

    #[test]
    fn simulated_timeline_never_decides_mirror_for_tablet_only() {
        for (phase, _) in simulated_tablet_only_timeline() {
            assert_ne!(activity_stage(phase), ActivityStage::Error);
            let steps = activity_steps(ShareMode::External, phase, true);
            assert!(
                steps.iter().any(|s| s.id == "blank"),
                "missing blank step at {phase}"
            );
            assert!(!steps.iter().any(|s| s.label.contains("镜像")), "{phase}");
        }
    }

    #[test]
    fn activity_steps_advance() {
        let prepare = activity_steps(ShareMode::External, "准备虚拟屏", true);
        assert_eq!(prepare[0].state, "current");
        assert_eq!(prepare[0].id, "prepare");

        let wait = activity_steps(ShareMode::External, "等待设备", true);
        assert_eq!(wait[0].state, "done");
        assert_eq!(wait[1].state, "current");

        let blank = activity_steps(ShareMode::External, "仅平板", true);
        assert_eq!(
            blank.iter().find(|s| s.id == "blank").unwrap().state,
            "current"
        );

        let stream = activity_steps(ShareMode::External, "编码", true);
        assert_eq!(stream.last().unwrap().state, "current");
        assert!(stream
            .iter()
            .take(stream.len() - 1)
            .all(|s| s.state == "done"));
    }

    #[test]
    fn titles_explain_the_current_action() {
        assert_eq!(activity_title(true, "准备虚拟屏"), "正在启用虚拟屏");
        assert_eq!(activity_title(true, "仅平板"), "正在关闭电脑屏");
        assert_eq!(activity_title(true, "编码"), "正在投屏");
        assert_eq!(activity_title(false, ""), "未开始共享");
    }

    #[test]
    fn glidex_dxgi_output_is_tablet_only_ready() {
        // DXGI already enumerated a GlideX / secondary head.
        assert_eq!(
            decide_after_virtual_prepare(ShareMode::External, true, "", true, false),
            VirtualPrepareOutcome::Ready
        );
        assert_eq!(
            decide_after_virtual_prepare(ShareMode::External, true, "", false, true),
            VirtualPrepareOutcome::Ready
        );
    }

    #[test]
    fn glidex_adapter_without_dxgi_output_still_needs_provision() {
        // Device Manager only — not a monitor. Keep trying LightingIdd / MttVDD.
        let out = decide_after_virtual_prepare(
            ShareMode::External,
            false,
            "DEVICE_NOT_FOUND",
            false,
            false,
        );
        assert!(matches!(out, VirtualPrepareOutcome::Abort { .. }));
        assert!(!matches!(out, VirtualPrepareOutcome::FallbackMirror { .. }));
    }

    #[test]
    fn missing_result_file_is_not_uac_cancelled() {
        assert_eq!(
            classify_provision_uac(true, 0, true, false, true, false),
            Err(ProvisionUacError::NoResult)
        );
        assert_eq!(
            classify_provision_uac(true, 0, true, false, true, false)
                .unwrap_err()
                .code(),
            "UAC_NO_RESULT"
        );
        assert_eq!(
            classify_provision_uac(false, 1223, false, false, false, false),
            Err(ProvisionUacError::Cancelled)
        );
        assert_eq!(
            classify_provision_uac(false, 5, false, false, false, false),
            Err(ProvisionUacError::AccessDenied)
        );
        assert_eq!(
            classify_provision_uac(true, 0, true, true, false, false),
            Err(ProvisionUacError::Timeout)
        );
        assert_eq!(
            classify_provision_uac(false, 2, false, false, false, false),
            Err(ProvisionUacError::ShellExecuteFailed)
        );
        assert_eq!(
            classify_provision_uac(true, 0, false, false, false, false),
            Err(ProvisionUacError::ShellExecuteFailed)
        );
        assert_eq!(
            classify_provision_uac(true, 0, true, false, false, false),
            Err(ProvisionUacError::WaitFailed)
        );
        assert_eq!(
            classify_provision_uac(true, 0, true, false, true, true),
            Ok(())
        );
    }

    #[test]
    fn last_error_is_hidden_when_activity_already_has_abort_copy() {
        let abort = tablet_only_abort_message("未找到虚拟显示设备。");
        assert!(!should_show_separate_last_error(
            &abort,
            &[&abort, "未开始共享"]
        ));
        assert!(should_show_separate_last_error(
            &abort,
            &["请连接你的设备，点击开始共享"]
        ));
        assert!(!should_show_separate_last_error("", &["x"]));
    }

    #[test]
    fn elevated_start_notice_says_no_extra_uac() {
        let tablet = share_start_notice(ShareMode::External, true);
        assert!(tablet.contains("不会再弹出"));
        assert!(tablet.contains("360"));
        assert!(!tablet.contains("请留意管理员确认"));
        let unelevated = share_start_notice(ShareMode::External, false);
        assert!(unelevated.contains("管理员确认"));
        assert!(virtual_driver_install_copy(true).contains("不会再弹出"));
        assert!(virtual_driver_install_copy(true).contains("360"));
    }

    #[test]
    fn killed_child_or_missing_result_is_never_uac_cancelled() {
        assert_eq!(
            classify_provision_finish(false, false, false, ""),
            Err(ProvisionFinishError::Interrupted)
        );
        assert_eq!(
            classify_provision_finish(false, true, false, ""),
            Err(ProvisionFinishError::Interrupted)
        );
        assert_eq!(
            classify_provision_finish(false, false, true, "garbage"),
            Err(ProvisionFinishError::Interrupted)
        );
        assert_eq!(
            classify_provision_finish(true, false, false, ""),
            Err(ProvisionFinishError::Timeout)
        );
        assert_eq!(
            classify_provision_finish(false, true, true, "OK|READY"),
            Ok(())
        );
        assert_eq!(
            classify_provision_finish(false, false, true, "FAIL|DEVICE_NOT_FOUND"),
            Ok(())
        );
        for kind in [
            ProvisionFinishError::Interrupted,
            ProvisionFinishError::NoResult,
            ProvisionFinishError::UnknownResult,
            ProvisionFinishError::Timeout,
        ] {
            assert_ne!(kind.code(), "UAC_CANCELLED", "{}", kind.code());
        }
    }

    #[test]
    fn strip_electron_ipc_prefix() {
        assert_eq!(
            strip_electron_ipc_error(
                "Error invoking remote method 'host:startShare': Error: 已取消管理员授权。扩展屏需要安装虚拟显示驱动，请在弹窗中点「是」。"
            ),
            "已取消管理员授权。扩展屏需要安装虚拟显示驱动，请在弹窗中点「是」。"
        );
        assert_eq!(
            strip_electron_ipc_error(
                "Error invoking remote method 'host:startShare': Error: 安装被中断（可能是 360/杀毒软件）。"
            ),
            "安装被中断（可能是 360/杀毒软件）。"
        );
        assert_eq!(strip_electron_ipc_error("仅平板需要虚拟屏"), "仅平板需要虚拟屏");
    }

    #[test]
    fn tablet_only_user_copy_never_promises_auto_mirror() {
        let abort = tablet_only_abort_message("未找到虚拟显示设备。");
        assert!(!abort.contains("自动镜像"));
        assert!(abort.contains("没有改用镜像"));
        assert!(!virtual_driver_install_copy(true).contains("自动镜像"));
        assert!(!share_start_notice(ShareMode::External, true).contains("自动镜像"));
        assert!(!share_start_notice(ShareMode::External, false).contains("自动镜像"));
    }

    #[test]
    fn elevated_interrupt_is_not_hidden_as_device_not_found() {
        assert!(should_surface_provision_interrupt("INSTALL_INTERRUPTED"));
        assert!(should_surface_provision_interrupt("FAIL|INSTALL_INTERRUPTED"));
        assert!(should_surface_provision_interrupt("DRIVER_NO_RESULT"));
        assert!(!should_surface_provision_interrupt("DEVICE_NOT_FOUND"));
        assert!(!should_surface_provision_interrupt("VDD_NO_MONITOR"));
        assert_eq!(
            choose_virtual_prepare_error(Some("INSTALL_INTERRUPTED"), Some("VDD_NO_MONITOR")),
            "INSTALL_INTERRUPTED"
        );
        assert_eq!(
            choose_virtual_prepare_error(Some("IDD_NO_MONITOR"), Some("VDD_NO_MONITOR")),
            "VDD_NO_MONITOR"
        );
        let out = decide_after_virtual_prepare(
            ShareMode::External,
            false,
            "INSTALL_INTERRUPTED",
            false,
            false,
        );
        match out {
            VirtualPrepareOutcome::Abort { reason } => {
                let human = crate::ui_text::human_last_error(&reason);
                assert!(human.contains("360") || human.contains("安全软件"));
                assert!(human.contains("中断"));
                assert!(!human.contains("未找到虚拟显示设备"));
                assert!(!human.contains("已取消"));
                let abort = tablet_only_abort_message(&human);
                assert!(abort.contains("没有改用镜像"));
                assert!(abort.contains("360") || abort.contains("安全软件"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn incomplete_idd_bundle_skips_install_and_is_not_device_not_found() {
        assert!(!should_attempt_idd_install(true, false));
        assert!(!should_attempt_idd_install(false, true));
        assert!(should_attempt_idd_install(true, true));
        assert_eq!(idd_bundle_incomplete_copy(), "安装包缺少虚拟屏驱动文件");
        assert!(!should_surface_provision_interrupt("BUNDLE_DLL_MISSING"));
        assert!(!should_surface_provision_interrupt("BUNDLE_INF_MISSING"));
        assert!(is_idd_bundle_incomplete("FAIL|BUNDLE_DLL_MISSING"));
        assert_eq!(
            choose_virtual_prepare_error(Some("BUNDLE_DLL_MISSING"), Some("DEVICE_NOT_FOUND")),
            "BUNDLE_DLL_MISSING"
        );
        assert_eq!(
            choose_virtual_prepare_error(Some("BUNDLE_DLL_MISSING"), Some("VDD_NO_MONITOR")),
            "BUNDLE_DLL_MISSING"
        );
        let human = crate::ui_text::human_last_error("BUNDLE_DLL_MISSING");
        assert_eq!(human, "安装包缺少虚拟屏驱动文件");
        assert!(!human.contains("未找到虚拟显示设备"));
        let abort = tablet_only_abort_message(&human);
        assert!(abort.contains("安装包缺少虚拟屏驱动文件"));
        assert!(!abort.contains("未找到虚拟显示设备"));
        assert!(!abort.contains("自动镜像"));
    }

    #[test]
    fn staged_inf_is_not_a_device() {
        assert!(driver_store_staged(0));
        assert!(driver_store_staged(259));
        assert!(!driver_store_staged(1));
        assert!(should_create_root_device_node(false));
        assert!(!should_create_root_device_node(true));
        assert!(!enable_device_exit_proves_presence(0));
        assert!(!enable_device_exit_proves_presence(259));
        // QA: oem90 in the store, no Root\MttVDD node — still need nefcon.
        assert!(should_create_root_device_node(
            first_ready_intended(&[], MTT_VDD_HWID).is_some()
        ));
    }

    fn mtt_started() -> PnpDeviceMatch {
        PnpDeviceMatch {
            instance_id: r"ROOT\MTTVDD\0000".into(),
            hardware_ids: vec![r"ROOT\MTTVDD".into()],
            friendly_name: "Virtual Display Driver".into(),
            class_name: "Display".into(),
            status: "Started".into(),
            problem_code: Some(0),
        }
    }

    #[test]
    fn glidex_and_generic_virtual_display_are_not_intended() {
        let glidex = PnpDeviceMatch {
            instance_id: r"PCI\VEN_1002&DEV_1681".into(),
            hardware_ids: vec!["PCI\\VEN_1002".into()],
            friendly_name: "ASUS GlideX Display".into(),
            class_name: "Display".into(),
            status: "OK".into(),
            problem_code: Some(0),
        };
        assert!(!is_intended_virtual_adapter(&glidex, MTT_VDD_HWID));
        assert!(!is_ready_virtual_adapter(&glidex, MTT_VDD_HWID));
        assert!(!is_intended_virtual_adapter(&glidex, LIGHTING_IDD_HWID));

        let generic = PnpDeviceMatch {
            instance_id: r"DISPLAY\DEFAULT\1".into(),
            hardware_ids: vec!["MONITOR\\DEFAULT".into()],
            friendly_name: "Virtual Display".into(),
            class_name: "Monitor".into(),
            status: "OK".into(),
            problem_code: Some(0),
        };
        assert!(!is_intended_virtual_adapter(&generic, MTT_VDD_HWID));
        assert!(!provision_ok_names_intended_device("OK|READY:Virtual_Display"));
        assert!(!provision_ok_names_intended_device("OK|READY:Virtual_Display_Driver"));
        assert!(!provision_ok_names_intended_device("OK|ENABLED:GlideX"));
    }

    #[test]
    fn root_mttvdd_and_lightingidd_are_intended_when_started() {
        let mtt = mtt_started();
        assert!(is_intended_virtual_adapter(&mtt, MTT_VDD_HWID));
        assert!(is_ready_virtual_adapter(&mtt, MTT_VDD_HWID));
        assert!(!is_intended_virtual_adapter(&mtt, LIGHTING_IDD_HWID));

        let idd = PnpDeviceMatch {
            instance_id: r"ROOT\LIGHTINGIDD\0000".into(),
            hardware_ids: vec![r"Root\LightingIdd".into()],
            friendly_name: "Lighting Virtual Display".into(),
            class_name: "System".into(),
            status: "OK".into(),
            problem_code: Some(0),
        };
        assert!(is_intended_virtual_adapter(&idd, LIGHTING_IDD_HWID));
        assert!(is_ready_virtual_adapter(&idd, LIGHTING_IDD_HWID));
    }

    #[test]
    fn problem_or_unknown_class_node_is_not_ready() {
        let problem = PnpDeviceMatch {
            instance_id: r"ROOT\MTTVDD\0000".into(),
            hardware_ids: vec![r"ROOT\MTTVDD".into()],
            friendly_name: "Virtual Display Driver".into(),
            class_name: "Unknown".into(),
            status: "Problem".into(),
            problem_code: Some(28),
        };
        assert!(is_intended_virtual_adapter(&problem, MTT_VDD_HWID));
        assert!(!is_ready_virtual_adapter(&problem, MTT_VDD_HWID));

        let unknown_class = PnpDeviceMatch {
            instance_id: r"ROOT\LIGHTINGIDD\0000".into(),
            hardware_ids: vec![r"ROOT\LIGHTINGIDD".into()],
            friendly_name: "Lighting Virtual Display".into(),
            class_name: "Unknown".into(),
            status: "Unknown".into(),
            problem_code: None,
        };
        assert!(!is_ready_virtual_adapter(&unknown_class, LIGHTING_IDD_HWID));
    }

    #[test]
    fn provision_ok_line_requires_intended_hwid() {
        assert!(provision_claimed_ok_without_device("OK|READY"));
        assert!(provision_claimed_ok_without_device("OK|ENABLED"));
        assert!(provision_claimed_ok_without_device("OK|READY:unknown"));
        assert!(provision_claimed_ok_without_device("OK|ENABLED:Virtual_Display"));
        assert!(!provision_claimed_ok_without_device(r"OK|READY:ROOT\MttVDD\0000"));
        assert!(!provision_claimed_ok_without_device("OK|ENABLED:ROOT_LightingIdd_0000"));
        assert!(!provision_claimed_ok_without_device("FAIL|DEVICE_NOT_FOUND"));
        assert!(provision_ok_names_intended_device("OK|READY:ROOT_MTTVDD_0000"));
        assert!(provision_ok_names_intended_device("OK|ENABLED:ROOT_IddSampleDriver_0000"));
        // classify_provision_finish still accepts a parseable OK| — that is
        // "not an interrupt", not "a real device exists".
        assert_eq!(
            classify_provision_finish(false, true, true, "OK|READY"),
            Ok(())
        );
        assert!(provision_claimed_ok_without_device("OK|READY"));
    }

    #[test]
    fn idd_native_full_does_not_ok_when_only_staged() {
        // QA: added=true from pnputil 0/259, enable-device exit 0, no node.
        let finish = finish_native_full(true, true, None, LIGHTING_IDD_HWID);
        assert_eq!(
            finish,
            NativeProvisionFinish::Fail("DEVICE_STILL_MISSING")
        );
        let finish = finish_native_enable_only(true, None, LIGHTING_IDD_HWID);
        assert_eq!(finish, NativeProvisionFinish::Fail("DEVICE_NOT_FOUND"));
        let mtt = mtt_started();
        assert_eq!(
            finish_native_full(true, true, Some(&mtt), MTT_VDD_HWID),
            NativeProvisionFinish::OkReady(r"ROOT\MTTVDD\0000".into())
        );
    }

    #[test]
    fn parse_pnputil_enum_distinguishes_missing_and_started() {
        assert!(parse_pnputil_enum_devices(
            "Microsoft PnP Utility\n\nNo devices were found on the system.\n"
        )
        .is_empty());

        let started = parse_pnputil_enum_devices(
            r#"
Microsoft PnP Utility

Instance ID:                ROOT\MTTVDD\0000
Device Description:         Virtual Display Driver
Class Name:                 Display
Class GUID:                 {4d36e968-e325-11ce-bfc1-08002be10318}
Manufacturer Name:          MikeTheTech
Status:                     Started
Driver Name:                oem90.inf
"#,
        );
        assert_eq!(started.len(), 1);
        assert!(first_ready_intended(&started, MTT_VDD_HWID).is_some());
        assert!(first_ready_intended(&started, LIGHTING_IDD_HWID).is_none());

        let problem = parse_pnputil_enum_devices(
            r#"
Instance ID:                ROOT\LIGHTINGIDD\0000
Device Description:         Lighting Virtual Display
Class Name:                 Unknown
Status:                     Problem
Problem Code:               28 (0x1C)
"#,
        );
        assert!(first_intended_adapter(&problem, LIGHTING_IDD_HWID).is_some());
        assert!(first_ready_intended(&problem, LIGHTING_IDD_HWID).is_none());
    }

    #[test]
    fn tablet_only_still_aborts_when_provision_lied_about_ok() {
        assert!(provision_claimed_ok_without_device("OK|ENABLED"));
        let out = decide_after_virtual_prepare(
            ShareMode::External,
            false,
            "DEVICE_STILL_MISSING",
            false,
            false,
        );
        assert!(matches!(out, VirtualPrepareOutcome::Abort { .. }));
        assert!(!matches!(out, VirtualPrepareOutcome::FallbackMirror { .. }));
    }
}
