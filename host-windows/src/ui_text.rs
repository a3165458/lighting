//! Beginner-facing copy and number formatting for the host window.
//!
//! Lives in the library (not next to the egui widgets) so the wording that
//! shields users from ports, adb and socket errors stays unit-tested on any
//! platform, including the Linux CI box that cannot link eframe.

/// Semantic color of a status line. The egui layer maps these to the palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Bad,
    Info,
    #[default]
    Muted,
}

/// Normalize the internal phase names into the handful the window shows.
pub fn display_phase(phase: &str) -> String {
    match phase {
        "" => "空闲".into(),
        "启动" | "启动中" => "监听".into(),
        "准备虚拟屏" | "启用驱动" => "正在启用虚拟屏".into(),
        "仅平板" => "正在关闭电脑屏".into(),
        "独立第二屏" => "正在适配平板".into(),
        "USB" | "USB 警告" | "等待" => "等待设备".into(),
        "回退" => "编码".into(),
        other => other.to_string(),
    }
}

pub fn metrics_line(frames: u64, bitrate_kbps: u32) -> String {
    let frames_s = if frames > 0 {
        frames.to_string()
    } else {
        "—".into()
    };
    let br_s = if bitrate_kbps > 0 {
        bitrate_kbps.to_string()
    } else {
        "—".into()
    };
    format!("已发送 {frames_s} 帧 · {br_s} kbps")
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let b = bytes as f64;
    let (value, unit) = if b < KB {
        return format!("{bytes} B");
    } else if b < MB {
        (b / KB, "KB")
    } else if b < GB {
        (b / MB, "MB")
    } else if b < TB {
        (b / GB, "GB")
    } else {
        (b / TB, "TB")
    };
    let decimals = if value < 10.0 {
        2
    } else if value < 100.0 {
        1
    } else {
        0
    };
    format!("{value:.decimals$} {unit}")
}

pub fn format_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

pub fn latency_text(latency_ms: u32) -> String {
    if latency_ms == 0 {
        "—".into()
    } else {
        format!("≈ {latency_ms} ms")
    }
}

pub fn loss_text(running: bool, loss_permille: u32) -> String {
    if !running {
        return "—".into();
    }
    if loss_permille == 0 {
        return "0%".into();
    }
    format!("{:.1}%", loss_permille as f32 / 10.0)
}

pub fn codec_text(live_codec: &str, prefer_hevc: bool) -> String {
    let live = live_codec.trim().to_lowercase();
    let hevc = if live.is_empty() {
        prefer_hevc
    } else {
        live.contains("hevc") || live.contains("h265")
    };
    if hevc {
        "HEVC（推荐）".into()
    } else {
        "AVC（兼容）".into()
    }
}

pub fn transport_text(running: bool, transport: &str) -> String {
    if !running {
        return "自适应优化".into();
    }
    if transport.contains("已就绪") {
        "USB 直连".into()
    } else if transport.contains("adb") || transport.contains("未检测") {
        "局域网".into()
    } else {
        "自适应优化".into()
    }
}

/// Loopback means the stream rides the USB tunnel, so show that instead of an
/// address the user never typed.
pub fn client_display_addr(addr: &str) -> String {
    let raw = addr.trim();
    let host = if let Some(end) = raw.find(']') {
        raw[1..end].to_string()
    } else if raw.matches(':').count() == 1 {
        raw.split(':').next().unwrap_or(raw).to_string()
    } else {
        raw.to_string()
    };
    if host.is_empty()
        || host == "::1"
        || host == "localhost"
        || host.starts_with("127.")
        || host.starts_with("::ffff:127.")
    {
        return "USB 直连".into();
    }
    host
}

pub fn peer_name(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        "平板".into()
    } else {
        name.to_string()
    }
}

pub fn connection_title(running: bool, phase: &str, client_name: &str) -> String {
    if is_streaming(phase) {
        return format!("已连接：{}", peer_name(client_name));
    }
    if !running {
        return "未开始共享".into();
    }
    if phase == "错误" {
        return "未能开始共享".into();
    }
    crate::share_flow::activity_title(true, phase)
}

pub fn display_choice_label(index: usize, primary: bool, width: u32, height: u32) -> String {
    let kind = if primary { "主显示器" } else { "扩展显示器" };
    format!("{kind} #{n}  ({width} × {height})", n = index + 1)
}
pub fn is_streaming(phase: &str) -> bool {
    matches!(phase, "已连接" | "编码" | "回退" | "共享中")
}

pub fn share_button_label(running: bool) -> String {
    if running {
        "停止共享".into()
    } else {
        "开始共享".into()
    }
}

/// Beginner copy when a USB device is ready but the Lighting client APK is missing.
pub fn client_app_missing_hint(can_install: bool) -> (String, Tone) {
    if can_install {
        (
            "已找到设备，但还没安装 Lighting 客户端。点下方「安装到平板」即可".into(),
            Tone::Warn,
        )
    } else {
        (
            "已找到设备，但还没安装 Lighting 客户端。请先把 APK 拷到电脑程序目录，或用 Android Studio 安装".into(),
            Tone::Warn,
        )
    }
}

pub fn client_app_installing_hint() -> String {
    "正在把 Lighting 安装到平板，请在平板上点「允许安装」…".into()
}

pub fn client_app_installed_ok() -> String {
    "客户端已安装。打开平板上的 Lighting，再点「开始共享」".into()
}

/// Footer health line: one glance answer to "is this working right now?".
pub fn health_text(running: bool, phase: &str, latency_ms: u32) -> (String, Tone) {
    if phase == "错误" {
        return ("连接异常，请看上面的提示".into(), Tone::Bad);
    }
    if is_streaming(phase) {
        return match latency_ms {
            0..=60 => ("连接良好".into(), Tone::Ok),
            61..=140 => ("连接一般".into(), Tone::Warn),
            _ => ("网络较慢，可降低画质".into(), Tone::Warn),
        };
    }
    if running {
        return ("等待平板连接".into(), Tone::Info);
    }
    ("未开始共享".into(), Tone::Muted)
}

pub fn humanize_transport(raw: &str) -> (String, Tone) {
    if raw.contains("已就绪") {
        ("USB 已就绪".into(), Tone::Ok)
    } else if raw.contains("失败") {
        ("请换数据线，并确认已点允许 USB 调试".into(), Tone::Warn)
    } else if raw.contains("未找到 adb") {
        (
            "未检测到 USB 驱动。请换数据线，或在高级里用局域网连接".into(),
            Tone::Warn,
        )
    } else if raw.contains("未检测") {
        ("未检测到设备。请检查数据线是否支持传数据".into(), Tone::Warn)
    } else {
        (raw.to_string(), Tone::Muted)
    }
}

pub fn human_detail_text(phase: &str, detail: &str) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        return String::new();
    }
    if phase == "准备虚拟屏" || phase == "仅平板" || phase == "独立第二屏" || phase == "启用驱动" {
        return detail.to_string();
    }
    let lower = detail.to_lowercase();
    if phase == "错误" {
        return human_last_error(detail);
    }
    // The connected detail is the peer socket address, which the card already
    // renders in friendly form.
    if phase == "已连接" {
        return "平板已连上".into();
    }
    // While encoding, the detail carries the negotiated pipeline; the metric
    // tiles show that already, so only surface the self-healing fallback.
    if phase == "编码" || phase == "回退" {
        if detail.contains("gdigrab") {
            return "画面已自动恢复".into();
        }
        return String::new();
    }
    if looks_like_bind_or_port(detail) {
        return String::new();
    }
    if lower.contains("adb reverse 失败") {
        return "请换数据线，或检查是否弹出 USB 调试允许".into();
    }
    if lower.contains("adb reverse") {
        return String::new();
    }
    if detail.contains("请在平板") {
        return "请在平板点「USB 一键连接」".into();
    }
    if detail.contains("未检测到已授权") {
        return "未检测到设备。请打开 USB 调试并点允许，或换一根能传数据的线".into();
    }
    if detail.contains("未找到 adb") {
        return "未检测到 USB 驱动。请换数据线，或在高级里用局域网连接".into();
    }
    if looks_technical(detail) {
        return String::new();
    }
    detail.to_string()
}

/// Map bundled virtual-display (MttVDD) error codes to beginner copy.
pub fn human_vdd_error(raw: &str) -> Option<String> {
    let upper = raw.to_uppercase();
    let code = raw
        .split_whitespace()
        .find(|t| t.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        .unwrap_or(raw);
    let code_upper = code.to_uppercase();

    let msg = if code_upper.contains("UAC_DENIED") {
        "需要管理员权限才能安装虚拟显示驱动。未提权时请在蓝底 UAC 窗口点「是」；已用管理员运行则不会再弹窗。"
    } else if code_upper.contains("BUNDLE_DLL_MISSING") || code_upper.contains("BUNDLE_INF_MISSING") {
        "安装包缺少虚拟屏驱动文件"
    } else if code_upper.contains("VDD_BUNDLE_MISSING") {
        "安装包缺少虚拟显示驱动文件。请从 GitHub 下载最新版 Lighting 便携版/安装包。"
    } else if code_upper.contains("VDD_SCRIPT_MISSING") {
        "虚拟显示驱动安装脚本缺失。请重新下载完整安装包。"
    } else if code_upper.contains("DRIVER_INSTALL_FAILED") {
        "虚拟显示驱动安装失败。请完全退出后右键 Lighting 选「以管理员身份运行」再试；仍失败可到 GitHub 手动安装 Virtual Display Driver。"
    } else if code_upper.contains("VDD_PIPE_DOWN") {
        "虚拟显示驱动未响应。请完全退出 Lighting 后以管理员身份运行再试（已是管理员时不会再弹窗）。"
    } else if code_upper.contains("IDD_NO_MONITOR") {
        "Lighting 自有虚拟显示驱动未能创建扩展屏。请完全退出后以管理员运行再试；开发机还需测试签名。"
    } else if code_upper.contains("DRIVER_SIGNATURE") {
        "虚拟显示驱动签名不被系统接受。开发请开 testsigning；发布需 Attestation 签名。"
    } else if code_upper.contains("VDD_NO_MONITOR") {
        "虚拟屏尚未出现。请完全退出 Lighting 后以管理员身份运行再试（已是管理员时不会再弹窗）。"
    } else if code_upper.contains("DEVICE_NOT_FOUND") || code_upper.contains("DEVICE_STILL_MISSING") {
        "未找到虚拟显示设备。请完全退出后以管理员身份运行再试。"
    } else if code_upper.contains("UAC_TIMEOUT") {
        "等了很久也没有管理员确认窗口。请完全退出 Lighting，右键选「以管理员身份运行」后再点开始共享（已是管理员时不会再弹窗）。"
    } else if code_upper.contains("INSTALL_INTERRUPTED") {
        "安装被安全软件中断，请在 360 里允许 Lighting / powershell / pnputil。已是管理员时不会再弹 UAC。"
    } else if code_upper.contains("INSTALL_TIMEOUT") {
        "驱动安装超时。请完全退出 Lighting 后以管理员身份运行再试。若弹出 360/杀毒软件请选允许。"
    } else if code_upper.contains("UAC_NO_RESULT") || code_upper.contains("DRIVER_NO_RESULT") {
        "没写出安装结果。若弹出 360/杀毒软件请选允许；仍失败请把 Lighting 加入白名单后重试。"
    } else if code_upper.contains("UAC_HIDDEN_HOST") {
        "主机进程没有可见窗口，无法弹出管理员确认。请再点开始共享并留意蓝底「用户账户控制」，或右键以管理员身份运行（已是管理员时不会再弹窗）。"
    } else if code_upper.contains("SHELLEXECUTE_FAILED") || code_upper.contains("UAC_WAIT_FAILED") {
        "无法唤起或等待管理员确认窗口。请完全退出 Lighting，右键选「以管理员身份运行」后再试（已是管理员时不会再弹窗）。"
    } else if code_upper.contains("UAC_CANCELLED") {
        "已取消管理员授权。扩展屏需要安装虚拟显示驱动，请在弹窗中点「是」。"
    } else if code_upper.contains("ACCESS_DENIED") {
        "权限不足，无法安装虚拟显示驱动。请右键 Lighting 选「以管理员身份运行」。"
    } else if code_upper.contains("PNP_QUERY_FAILED") {
        "无法查询显示设备。请重启 Lighting 并以管理员身份运行后再试。"
    } else if code_upper.contains("BUNDLE_DIR_MISSING") {
        "找不到虚拟显示驱动目录。请重新下载完整安装包。"
    } else if code_upper.contains("UNEXPECTED") || code_upper.starts_with("ERR_") {
        "虚拟显示驱动安装遇到未知错误。请完全退出后以管理员身份运行 Lighting；仍失败可到 GitHub 手动安装 Virtual Display Driver。"
    } else if code_upper.contains("LAUNCHER_FAILED") || code_upper.contains("UNKNOWN_RESULT") {
        "没写出安装结果，或安装程序未能启动（可能是 360/杀毒软件拦了）。请允许后完全退出再试；已是管理员时不会再弹 UAC。"
    } else if upper.contains("静默安装")
        || upper.contains("NEFCON")
        || upper.contains("EXITCODE")
        || raw.contains('�')
    {
        "虚拟显示驱动安装失败（v0.1.7 及更早版本已知问题）。请升级到 v0.1.8 或更新版，并以管理员身份运行。"
    } else {
        return None;
    };
    Some(msg.into())
}

pub fn human_last_error(raw: &str) -> String {
    if let Some(vdd) = human_vdd_error(raw) {
        return vdd;
    }
    let lower = raw.to_lowercase();
    if raw.contains("找不到 adb") || lower.contains("adb.exe") {
        return "未检测到 USB 驱动。请安装平台工具，或换一根能传数据的线。".into();
    }
    if raw.contains("没有可用显示器") {
        return "没有可用显示器".into();
    }
    if raw.contains("扩展屏尚未就绪") {
        return raw.lines().next().unwrap_or(raw).to_string();
    }
    if looks_like_bind_or_port(raw) {
        return "无法开始共享，请稍后重试".into();
    }
    raw.lines().next().unwrap_or(raw).to_string()
}

pub fn looks_like_bind_or_port(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("17400")
        || lower.contains("0.0.0.0")
        || lower.contains("127.0.0.1")
        || lower.contains("connection refused")
        || text.contains("绑定")
}

pub fn looks_technical(text: &str) -> bool {
    looks_like_bind_or_port(text) || text.contains("adb reverse") || text.contains("tcp:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_phase_normalizes_share_states() {
        assert_eq!(display_phase(""), "空闲");
        assert_eq!(display_phase("启动"), "监听");
        assert_eq!(display_phase("启动中"), "监听");
        assert_eq!(display_phase("监听"), "监听");
        assert_eq!(display_phase("USB"), "等待设备");
        assert_eq!(display_phase("USB 警告"), "等待设备");
        assert_eq!(display_phase("等待"), "等待设备");
        assert_eq!(display_phase("等待设备"), "等待设备");
        assert_eq!(display_phase("已连接"), "已连接");
        assert_eq!(display_phase("编码"), "编码");
        assert_eq!(display_phase("回退"), "编码");
        assert_eq!(display_phase("准备虚拟屏"), "正在启用虚拟屏");
        assert_eq!(display_phase("仅平板"), "正在关闭电脑屏");
        assert_eq!(display_phase("错误"), "错误");
        assert_eq!(display_phase("已停止"), "已停止");
    }

    #[test]
    fn metrics_line_shows_frames_and_bitrate() {
        assert_eq!(metrics_line(0, 0), "已发送 — 帧 · — kbps");
        assert_eq!(metrics_line(12, 0), "已发送 12 帧 · — kbps");
        assert_eq!(metrics_line(0, 18000), "已发送 — 帧 · 18000 kbps");
        assert_eq!(metrics_line(90, 18000), "已发送 90 帧 · 18000 kbps");
    }

    #[test]
    fn bytes_read_like_a_download_counter() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(900), "900 B");
        assert_eq!(format_bytes(1_342_177_280), "1.25 GB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.00 MB");
        assert_eq!(format_bytes(512 * 1024 * 1024), "512 MB");
    }

    #[test]
    fn duration_is_zero_padded() {
        assert_eq!(format_duration(0), "00:00:00");
        assert_eq!(format_duration(765), "00:12:45");
        assert_eq!(format_duration(3_725), "01:02:05");
    }

    #[test]
    fn latency_and_loss_stay_blank_before_data() {
        assert_eq!(latency_text(0), "—");
        assert_eq!(latency_text(28), "≈ 28 ms");
        assert_eq!(loss_text(false, 0), "—");
        assert_eq!(loss_text(true, 0), "0%");
        assert_eq!(loss_text(true, 25), "2.5%");
    }

    #[test]
    fn codec_text_prefers_live_negotiation() {
        assert_eq!(codec_text("hevc", false), "HEVC（推荐）");
        assert_eq!(codec_text("avc", true), "AVC（兼容）");
        assert_eq!(codec_text("", true), "HEVC（推荐）");
        assert_eq!(codec_text("", false), "AVC（兼容）");
    }

    #[test]
    fn transport_text_hides_adb_jargon() {
        assert_eq!(
            transport_text(true, "USB · adb reverse 已就绪（R52N）"),
            "USB 直连"
        );
        assert_eq!(
            transport_text(true, "未找到 adb · 仅局域网可用（平板填电脑 IP）"),
            "局域网"
        );
        assert_eq!(transport_text(false, "USB · adb reverse 已就绪"), "自适应优化");
    }

    #[test]
    fn loopback_peer_shows_as_usb() {
        assert_eq!(client_display_addr("127.0.0.1:53812"), "USB 直连");
        assert_eq!(client_display_addr("[::1]:53812"), "USB 直连");
        assert_eq!(client_display_addr("192.168.1.105:53812"), "192.168.1.105");
        assert_eq!(client_display_addr(""), "USB 直连");
    }

    #[test]
    fn connection_title_tracks_phase() {
        assert_eq!(
            connection_title(true, "编码", "Google Pixel Tablet"),
            "已连接：Google Pixel Tablet"
        );
        assert_eq!(connection_title(true, "编码", ""), "已连接：平板");
        assert_eq!(connection_title(true, "等待设备", ""), "等待平板连接");
        assert_eq!(connection_title(true, "准备虚拟屏", ""), "正在启用虚拟屏");
        assert_eq!(connection_title(true, "仅平板", ""), "正在关闭电脑屏");
        assert_eq!(connection_title(true, "错误", ""), "未能开始共享");
        assert_eq!(connection_title(false, "已停止", ""), "未开始共享");
    }

    #[test]
    fn health_text_reacts_to_latency() {
        assert_eq!(health_text(true, "编码", 28).0, "连接良好");
        assert_eq!(health_text(true, "编码", 90).0, "连接一般");
        assert_eq!(health_text(true, "编码", 400).1, Tone::Warn);
        assert_eq!(health_text(true, "等待设备", 0).1, Tone::Info);
        assert_eq!(health_text(false, "已停止", 0).1, Tone::Muted);
        assert_eq!(health_text(true, "错误", 0).1, Tone::Bad);
    }

    #[test]
    fn share_button_toggles_copy() {
        assert_eq!(share_button_label(false), "开始共享");
        assert_eq!(share_button_label(true), "停止共享");
    }

    #[test]
    fn client_app_missing_copy_is_beginner_friendly() {
        let (with_apk, tone) = client_app_missing_hint(true);
        assert!(with_apk.contains("安装到平板"));
        assert_eq!(tone, Tone::Warn);
        let (no_apk, _) = client_app_missing_hint(false);
        assert!(no_apk.contains("Lighting 客户端"));
        assert!(!no_apk.contains("adb"));
        assert!(!client_app_installing_hint().contains("adb"));
        assert!(client_app_installed_ok().contains("开始共享"));
    }

    #[test]
    fn transport_copy_never_leaks_adb() {
        assert_eq!(
            humanize_transport("USB · adb reverse 已就绪（R52N）"),
            ("USB 已就绪".into(), Tone::Ok)
        );
        assert_eq!(
            humanize_transport("USB · adb reverse 失败，可改用 Wi-Fi（平板填电脑 IP）").1,
            Tone::Warn
        );
        assert!(!humanize_transport("USB · adb reverse 失败").0.contains("adb"));
        assert!(!humanize_transport("未找到 adb · 仅局域网可用").0.contains("adb ·"));
    }

    #[test]
    fn detail_copy_stays_human() {
        assert_eq!(human_detail_text("监听", "绑定 0.0.0.0:17400"), "");
        assert_eq!(
            human_detail_text("等待设备", "正在执行 adb reverse（R52N）"),
            ""
        );
        assert_eq!(
            human_detail_text("等待设备", "adb reverse 失败，仍可走局域网：boom"),
            "请换数据线，或检查是否弹出 USB 调试允许"
        );
        assert_eq!(
            human_detail_text("等待设备", "请在平板上打开 Lighting 并连接"),
            "请在平板点「USB 一键连接」"
        );
        assert_eq!(human_detail_text("已连接", "192.168.1.105:53812"), "平板已连上");
        assert_eq!(human_detail_text("已连接", "127.0.0.1:53812"), "平板已连上");
        assert_eq!(
            human_detail_text("编码", "avc 1920×1080@60 18000 kbps + 音频 [qcom 硬解]"),
            ""
        );
        assert_eq!(
            human_detail_text("编码", "gdigrab 已重发 codec-config + IDR"),
            "画面已自动恢复"
        );
        assert_eq!(
            human_detail_text(
                "准备虚拟屏",
                crate::share_flow::virtual_driver_install_copy(true)
            ),
            crate::share_flow::virtual_driver_install_copy(true)
        );
    }

    #[test]
    fn last_error_copy_stays_human() {
        assert_eq!(
            human_last_error("找不到 adb.exe，请安装 platform-tools"),
            "未检测到 USB 驱动。请安装平台工具，或换一根能传数据的线。"
        );
        assert_eq!(human_last_error("没有可用显示器"), "没有可用显示器");
        assert_eq!(
            human_last_error("绑定端口: os error 10048"),
            "无法开始共享，请稍后重试"
        );
        assert!(human_vdd_error("VDD_PIPE_DOWN").unwrap().contains("未响应"));
        assert!(human_vdd_error("UNEXPECTED").unwrap().contains("未知错误"));
        assert!(human_vdd_error("UAC_CANCELLED").unwrap().contains("取消"));
        assert!(!human_vdd_error("UAC_NO_RESULT").unwrap().contains("取消"));
        assert!(!human_vdd_error("DRIVER_NO_RESULT").unwrap().contains("取消"));
        assert!(!human_vdd_error("DRIVER_UNKNOWN_RESULT").unwrap().contains("取消"));
        assert!(!human_vdd_error("INSTALL_TIMEOUT").unwrap().contains("已取消"));
        assert!(!human_vdd_error("INSTALL_INTERRUPTED").unwrap().contains("已取消"));
        assert!(!human_vdd_error("INSTALL_INTERRUPTED").unwrap().contains("未找到虚拟显示设备"));
        assert!(human_vdd_error("INSTALL_INTERRUPTED").unwrap().contains("360"));
        assert!(human_vdd_error("INSTALL_INTERRUPTED").unwrap().contains("安全软件"));
        assert!(human_vdd_error("DRIVER_NO_RESULT").unwrap().contains("没写出"));
        assert!(human_vdd_error("UAC_NO_RESULT").unwrap().contains("没写出"));
        assert!(human_vdd_error("UAC_HIDDEN_HOST").unwrap().contains("可见窗口"));
        assert!(human_vdd_error("SHELLEXECUTE_FAILED").unwrap().contains("无法唤起"));
        assert!(human_vdd_error("UAC_WAIT_FAILED").unwrap().contains("管理员确认"));
        assert!(human_vdd_error("FAIL|UAC_DENIED").unwrap().contains("管理员"));
        assert!(human_vdd_error("ACCESS_DENIED").unwrap().contains("权限不足"));
        assert!(human_vdd_error("DRIVER_SIGNATURE").unwrap().contains("签名"));
        assert!(human_vdd_error("UAC_TIMEOUT").unwrap().contains("以管理员身份运行"));
        assert!(human_vdd_error("INSTALL_TIMEOUT").unwrap().contains("超时"));
        assert!(human_vdd_error("DRIVER_LAUNCHER_FAILED").unwrap().contains("360"));
        assert!(!human_vdd_error("DRIVER_LAUNCHER_FAILED").unwrap().contains("已取消"));
        assert!(human_last_error("VDD_BUNDLE_MISSING").contains("缺少"));
        assert_eq!(
            human_vdd_error("BUNDLE_DLL_MISSING").unwrap(),
            "安装包缺少虚拟屏驱动文件"
        );
        assert_eq!(
            human_vdd_error("BUNDLE_INF_MISSING").unwrap(),
            "安装包缺少虚拟屏驱动文件"
        );
        assert!(!human_vdd_error("BUNDLE_DLL_MISSING")
            .unwrap()
            .contains("未找到虚拟显示设备"));
        assert!(looks_like_bind_or_port("connection refused"));
        assert!(looks_technical("adb reverse tcp:17400"));
        assert!(!looks_technical("没有可用显示器"));
    }

    #[test]
    fn display_choice_label_matches_mock() {
        assert_eq!(
            display_choice_label(1, false, 2560, 1600),
            "扩展显示器 #2  (2560 × 1600)"
        );
        assert_eq!(
            display_choice_label(0, true, 1920, 1080),
            "主显示器 #1  (1920 × 1080)"
        );
    }
}
