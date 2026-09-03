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
}
