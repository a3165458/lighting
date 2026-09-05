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
    Abort { reason: String },
}

/// Extension failure must remain a failure, never an unrequested mirror stream.
pub fn decide_after_virtual_prepare(
    mode: ShareMode,
    ensure_ok: bool,
    ensure_err: &str,
    has_secondary: bool,
    has_virtual: bool,
) -> VirtualPrepareOutcome {
    let present = has_secondary || (mode == ShareMode::External && has_virtual);
    if ensure_ok && present {
        return VirtualPrepareOutcome::Ready;
    }
    let reason = if !ensure_ok && !ensure_err.trim().is_empty() {
        ensure_err.trim().to_string()
    } else {
        "虚拟屏未能创建（驱动未就绪）".into()
    };
    VirtualPrepareOutcome::Abort { reason }
}

pub fn virtual_prepare_abort_message(reason: &str) -> String {
    format!("扩展屏未就绪，未改用镜像。{reason} 请保留错误详情；如需镜像，请手动选择「镜像主屏」。")
}

/// Copy shown while installing the virtual display driver.
pub fn virtual_driver_install_copy(already_admin: bool) -> &'static str {
    if already_admin {
        "已是管理员，正在直接安装虚拟显示驱动（不会再弹出确认窗口）…"
    } else {
        "需要安装驱动。请在蓝底「用户账户控制」窗口点「是」；没有弹窗时请看任务栏是否在闪。"
    }
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
        "准备虚拟屏" | "启用驱动" | "启动" | "启动中" => ActivityStage::PrepareVirtual,
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
        (
            "准备虚拟屏",
            "正在启用虚拟显示驱动（可能弹出管理员确认）…",
        ),
        ("准备虚拟屏", "正在等待虚拟屏出现…"),
        ("等待设备", "请在平板上打开 Lighting 并连接"),
        (
            "独立第二屏",
            "正在把虚拟屏设为平板分辨率 1920×1200",
        ),
        ("仅平板", "正在关闭电脑屏（Win+P 仅第二屏幕）…"),
        ("编码", "正在推流"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tablet_only_never_falls_back_to_mirror() {
        let out = decide_after_virtual_prepare(
            ShareMode::External,
            false,
            "UAC_CANCELLED",
            false,
            false,
        );
        assert!(
            matches!(out, VirtualPrepareOutcome::Abort { .. }),
            "{out:?}"
        );
        let out = decide_after_virtual_prepare(
            ShareMode::External,
            true,
            "",
            false,
            false,
        );
        assert!(matches!(out, VirtualPrepareOutcome::Abort { .. }));
    }

    #[test]
    fn extend_failure_preserves_driver_error_without_mirroring() {
        let out = decide_after_virtual_prepare(ShareMode::Extend, false, "DEVICE_STILL_MISSING", false, false);
        assert_eq!(out, VirtualPrepareOutcome::Abort { reason: "DEVICE_STILL_MISSING".into() });
        assert!(matches!(
            decide_after_virtual_prepare(ShareMode::Extend, true, "", false, true),
            VirtualPrepareOutcome::Abort { .. }
        ));
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
    fn abort_copy_keeps_the_driver_error_and_refuses_mirror() {
        let msg = virtual_prepare_abort_message("虚拟显示设备未创建成功。 [DEVICE_STILL_MISSING]");
        assert!(msg.contains("未改用镜像"));
        assert!(msg.contains("DEVICE_STILL_MISSING"));
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
            assert!(
                !steps.iter().any(|s| s.label.contains("镜像")),
                "{phase}"
            );
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
        assert_eq!(blank.iter().find(|s| s.id == "blank").unwrap().state, "current");

        let stream = activity_steps(ShareMode::External, "编码", true);
        assert_eq!(stream.last().unwrap().state, "current");
        assert!(stream.iter().take(stream.len() - 1).all(|s| s.state == "done"));
    }

    #[test]
    fn titles_explain_the_current_action() {
        assert_eq!(activity_title(true, "准备虚拟屏"), "正在启用虚拟屏");
        assert_eq!(activity_title(true, "仅平板"), "正在关闭电脑屏");
        assert_eq!(activity_title(true, "编码"), "正在投屏");
        assert_eq!(activity_title(false, ""), "未开始共享");
    }
}
