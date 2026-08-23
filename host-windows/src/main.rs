mod adb;
mod audio;
mod displays;
mod encoder;
mod input;
mod protocol;
mod session;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use session::{display_phase, live_transport, metrics_line, SessionRequest, SessionStatus};
use tracing_subscriber::fmt::writer::BoxMakeWriter;

fn enable_dpi_awareness() {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
}

fn log_writer() -> BoxMakeWriter {
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("lighting-host.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("lighting-host.log"));
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => {
            eprintln!("Lighting 日志: {}", path.display());
            BoxMakeWriter::new(std::sync::Mutex::new(file))
        }
        Err(_) => BoxMakeWriter::new(std::io::stderr),
    }
}

fn main() -> eframe::Result<()> {
    enable_dpi_awareness();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lighting_host=info,warn".into()),
        )
        .with_writer(log_writer())
        .with_ansi(false)
        .init();

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([540.0, 640.0])
            .with_min_inner_size([440.0, 500.0])
            .with_title("Lighting 副屏"),
        ..Default::default()
    };
    eframe::run_native(
        "Lighting",
        native,
        Box::new(|cc| {
            install_cjk_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_pixels_per_point(1.15);
            Ok(Box::new(LightingApp::new()))
        }),
    )
}

fn install_cjk_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyh.ttf",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\NotoSansSC-Regular.otf",
    ];
    let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
        tracing::warn!("未找到中文字体，界面汉字可能显示为方框");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".into(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "cjk".into());
    }
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        family.insert(0, "cjk".into());
    }
    ctx.set_fonts(fonts);
}

struct LightingApp {
    rt: tokio::runtime::Runtime,
    displays: Vec<displays::DisplayInfo>,
    devices: Vec<adb::AdbDevice>,
    selected_display: usize,
    selected_device: usize,
    prefer_hevc: bool,
    send_audio: bool,
    bitrate_kbps: u32,
    fps: u32,
    max_res: MaxRes,
    status: Arc<Mutex<SessionStatus>>,
    stop: Arc<AtomicBool>,
    running: bool,
    last_error: String,
    adb_path: String,
    bind_host: String,
    bind_port: u16,
    last_poll: Instant,
}

#[derive(Clone, Copy, PartialEq)]
enum MaxRes {
    Device,
    Balanced,
    Smooth,
    Fhd,
    Uhd2k,
}

impl LightingApp {
    fn new() -> Self {
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        let displays = displays::list_displays().unwrap_or_default();
        let selected_display = displays.iter().position(|d| !d.primary).unwrap_or(0);
        let mut app = Self {
            rt,
            displays,
            devices: Vec::new(),
            selected_display,
            selected_device: 0,
            prefer_hevc: false,
            send_audio: true,
            bitrate_kbps: 25_000,
            fps: 60,
            max_res: MaxRes::Device,
            status: Arc::new(Mutex::new(SessionStatus::default())),
            stop: Arc::new(AtomicBool::new(false)),
            running: false,
            last_error: String::new(),
            adb_path: String::new(),
            bind_host: "0.0.0.0".into(),
            bind_port: protocol::PORT,
            last_poll: Instant::now(),
        };
        app.refresh_devices();
        app
    }

    fn refresh_displays(&mut self) {
        match displays::list_displays() {
            Ok(list) => {
                self.displays = list;
                if self.selected_display >= self.displays.len() {
                    self.selected_display = 0;
                }
            }
            Err(err) => self.last_error = format!("{err:#}"),
        }
    }

    fn refresh_devices(&mut self) {
        let adb = match adb::find_adb() {
            Ok(p) => {
                self.adb_path = p.display().to_string();
                p
            }
            Err(err) => {
                self.adb_path.clear();
                self.last_error = format!("{err:#}");
                self.devices.clear();
                return;
            }
        };
        match self.rt.block_on(adb::list_devices(&adb)) {
            Ok(list) => {
                self.devices = list;
                self.select_usb_device();
                if self.last_error.contains("adb") || self.last_error.contains("找不到") {
                    self.last_error.clear();
                }
            }
            Err(err) => self.last_error = format!("{err:#}"),
        }
    }

    fn select_usb_device(&mut self) {
        let ready: Vec<usize> = self
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| d.state == "device")
            .map(|(i, _)| i)
            .collect();
        if ready.len() == 1 {
            self.selected_device = ready[0];
        } else if self.selected_device >= self.devices.len() {
            self.selected_device = 0;
        }
    }

    fn maybe_poll_devices(&mut self) {
        if self.running {
            return;
        }
        if self.last_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_poll = Instant::now();
        self.refresh_devices();
    }

    fn start(&mut self) {
        if self.displays.is_empty() {
            self.last_error = "没有可用显示器".into();
            return;
        }
        let (match_device, scale, max_width, max_height) = match self.max_res {
            MaxRes::Device => (true, 1.0, 3840, 2560),
            MaxRes::Balanced => (true, 0.75, 3840, 2560),
            MaxRes::Smooth => (true, 0.55, 3840, 2560),
            MaxRes::Fhd => (false, 1.0, 1920, 1080),
            MaxRes::Uhd2k => (false, 1.0, 2560, 1440),
        };
        let serial = self
            .devices
            .get(self.selected_device)
            .filter(|d| d.state == "device")
            .map(|d| d.serial.clone())
            .or_else(|| {
                self.devices
                    .iter()
                    .find(|d| d.state == "device")
                    .map(|d| d.serial.clone())
            });
        let bind_host = self.bind_host.trim();
        let bind_host = if bind_host.is_empty() {
            "0.0.0.0"
        } else {
            bind_host
        };
        let bind_port = if self.bind_port == 0 {
            protocol::PORT
        } else {
            self.bind_port
        };
        let req = SessionRequest {
            display_index: self.selected_display,
            device_serial: serial,
            bind: format!("{bind_host}:{bind_port}"),
            prefer_hevc: self.prefer_hevc,
            bitrate_kbps: self.bitrate_kbps,
            fps: self.fps,
            max_width,
            max_height,
            match_device,
            scale,
            send_audio: self.send_audio,
        };
        self.stop.store(false, Ordering::Relaxed);
        if let Ok(mut s) = self.status.lock() {
            *s = SessionStatus {
                running: true,
                phase: "启动中".into(),
                ..Default::default()
            };
        }
        let status = self.status.clone();
        let stop = self.stop.clone();
        self.rt.spawn(session::run_session(req, status, stop));
        self.running = true;
    }

    fn stop_session(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.running = false;
        if let Ok(mut s) = self.status.lock() {
            s.transport.clear();
            s.bitrate_kbps = 0;
            s.frames = 0;
            s.detail.clear();
        }
    }
}

impl eframe::App for LightingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        self.maybe_poll_devices();
        let snap = self
            .status
            .lock()
            .ok()
            .map(|s| s.clone())
            .unwrap_or_default();
        if self.running && !snap.running && snap.phase == "错误" {
            self.running = false;
        }
        if snap.phase == "已停止" {
            self.running = false;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Lighting 副屏");
            ui.label("插上数据线，点「开始共享」，平板再点「USB 一键连接」。");
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let start = ui.add_enabled(
                    !snap.running,
                    egui::Button::new(egui::RichText::new("开始共享").size(18.0))
                        .min_size(egui::vec2(168.0, 36.0)),
                );
                if start.clicked() {
                    self.start();
                }
                let stop = ui.add_enabled(
                    snap.running,
                    egui::Button::new("停止").min_size(egui::vec2(72.0, 36.0)),
                );
                if stop.clicked() {
                    self.stop_session();
                }
                if ui.button("刷新").clicked() {
                    self.refresh_displays();
                    self.refresh_devices();
                }
            });

            ui.add_space(8.0);
            let (transport, transport_color) =
                usb_transport_line(&snap, &self.adb_path, &self.devices);
            ui.colored_label(transport_color, transport);

            let ready: Vec<&adb::AdbDevice> = self
                .devices
                .iter()
                .filter(|d| d.state == "device")
                .collect();
            if ready.len() > 1 {
                ui.add_space(4.0);
                ui.label("检测到多台设备，请选一台：");
                device_combo(ui, "device_main", &self.devices, &mut self.selected_device);
            }

            ui.add_space(8.0);
            ui.label("要投出的显示器（扩展模式请先安装虚拟屏并选中副屏）：");
            if self.displays.is_empty() {
                ui.colored_label(egui::Color32::YELLOW, "未枚举到显示器");
            } else {
                egui::ComboBox::from_id_salt("display")
                    .selected_text(
                        self.displays
                            .get(self.selected_display)
                            .map(|d| d.label())
                            .unwrap_or_default(),
                    )
                    .show_ui(ui, |ui| {
                        for (i, d) in self.displays.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_display, i, d.label());
                        }
                    });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.label("输出分辨率（自动不超过该机硬解上限与对齐要求）");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.max_res, MaxRes::Device, "匹配设备");
                ui.radio_value(&mut self.max_res, MaxRes::Balanced, "平衡 75%");
                ui.radio_value(&mut self.max_res, MaxRes::Smooth, "流畅 55%");
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.max_res, MaxRes::Fhd, "强制 1080p");
                ui.radio_value(&mut self.max_res, MaxRes::Uhd2k, "强制 2K");
            });
            ui.horizontal(|ui| {
                ui.label("帧率");
                ui.add(egui::Slider::new(&mut self.fps, 30..=120).suffix(" fps"));
            });
            ui.horizontal(|ui| {
                ui.label("码率上限");
                ui.add(egui::Slider::new(&mut self.bitrate_kbps, 5_000..=40_000).suffix(" kbps"));
            });
            ui.checkbox(&mut self.send_audio, "同步传输系统声音（桌面音频）");
            ui.checkbox(&mut self.prefer_hevc, "优先 HEVC（设备支持时，文字更清晰）");

            ui.add_space(10.0);
            ui.separator();
            let phase = display_phase(&snap.phase);
            ui.horizontal(|ui| {
                ui.strong("阶段");
                ui.colored_label(phase_color(&phase), &phase);
            });
            let detail = human_detail(&snap);
            ui.label(if detail.is_empty() { "—" } else { detail.as_str() });
            ui.label(metrics_line(snap.frames, snap.bitrate_kbps));
            if !self.last_error.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 80, 80),
                    human_last_error(&self.last_error),
                );
            }

            ui.add_space(8.0);
            egui::CollapsingHeader::new("高级")
                .default_open(false)
                .show(ui, |ui| {
                    ui.label("局域网绑定（一般不用改）");
                    ui.horizontal(|ui| {
                        ui.label("地址");
                        ui.text_edit_singleline(&mut self.bind_host);
                    });
                    ui.horizontal(|ui| {
                        ui.label("端口");
                        ui.add(egui::DragValue::new(&mut self.bind_port).range(1..=65535));
                    });
                    ui.add_space(6.0);
                    ui.label("Android 设备");
                    if self.devices.is_empty() {
                        ui.weak("未检测到设备");
                    } else {
                        device_combo(ui, "device_advanced", &self.devices, &mut self.selected_device);
                    }
                    if !self.adb_path.is_empty() {
                        ui.weak(format!("adb：{}", self.adb_path));
                    }
                    if !self.last_error.is_empty() {
                        ui.collapsing("详情", |ui| {
                            ui.weak(&self.last_error);
                        });
                    }
                    if ui.button("刷新显示器 / 设备").clicked() {
                        self.refresh_displays();
                        self.refresh_devices();
                    }
                });

            ui.add_space(12.0);
            ui.weak("扩展屏：winget install VirtualDrivers.Virtual-Display-Driver ，然后在 Windows 显示设置里设为「扩展」并选这块虚拟屏。");
            ui.weak("平板触控：单击/拖动、长按右键、双指滚动。局域网请在两边的「高级」里填写。");
        });
    }
}

fn device_combo(ui: &mut egui::Ui, id: &'static str, devices: &[adb::AdbDevice], selected: &mut usize) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(
            devices
                .get(*selected)
                .map(|d| d.label())
                .unwrap_or_default(),
        )
        .show_ui(ui, |ui| {
            for (i, d) in devices.iter().enumerate() {
                ui.selectable_value(selected, i, d.label());
            }
        });
}

fn phase_color(phase: &str) -> egui::Color32 {
    match phase {
        "编码" | "已连接" | "共享中" => egui::Color32::from_rgb(90, 200, 120),
        "错误" | "出错" => egui::Color32::from_rgb(220, 80, 80),
        "已停止" | "空闲" => egui::Color32::from_rgb(170, 170, 170),
        "监听" | "等待设备" | "正在准备" => egui::Color32::from_rgb(230, 190, 80),
        _ => egui::Color32::from_rgb(200, 200, 200),
    }
}

fn humanize_session_transport(raw: &str) -> (String, egui::Color32) {
    let warn = egui::Color32::from_rgb(230, 180, 80);
    let ok = egui::Color32::from_rgb(90, 200, 120);
    if raw.contains("已就绪") {
        ("USB 已就绪".into(), ok)
    } else if raw.contains("失败") {
        ("请换数据线，并确认已点允许 USB 调试".into(), warn)
    } else if raw.contains("未找到 adb") {
        ("未检测到 USB 驱动。请换数据线，或在高级里用局域网连接".into(), warn)
    } else if raw.contains("未检测") {
        ("未检测到设备。请检查数据线是否支持传数据".into(), warn)
    } else {
        (raw.to_string(), egui::Color32::GRAY)
    }
}

fn usb_transport_line(
    snap: &SessionStatus,
    adb_path: &str,
    devices: &[adb::AdbDevice],
) -> (String, egui::Color32) {
    if let Some(raw) = live_transport(snap.running, &snap.transport) {
        return humanize_session_transport(raw);
    }
    let ready = devices.iter().filter(|d| d.state == "device").count();
    let pending = devices
        .iter()
        .any(|d| d.state == "unauthorized" || d.state == "offline");
    let warn = egui::Color32::from_rgb(230, 180, 80);
    let ok = egui::Color32::from_rgb(90, 200, 120);
    if adb_path.is_empty() {
        return ("未检测到 USB 驱动。请换数据线，或在高级里用局域网连接".into(), warn);
    }
    if pending && ready == 0 {
        return ("请打开 USB 调试并点允许".into(), warn);
    }
    if ready == 1 {
        let name = devices
            .iter()
            .find(|d| d.state == "device")
            .map(|d| d.serial.as_str())
            .unwrap_or("");
        return (format!("已找到设备，将自动连接 · {name}"), ok);
    }
    if ready > 1 {
        return ("检测到多台设备，请选择一台".into(), egui::Color32::WHITE);
    }
    ("未检测到设备。请检查数据线是否支持传数据".into(), warn)
}

fn human_detail(snap: &SessionStatus) -> String {
    let detail = snap.detail.trim();
    if detail.is_empty() {
        return String::new();
    }
    let lower = detail.to_lowercase();
    if snap.phase == "错误" {
        return human_last_error(detail);
    }
    if looks_like_bind_or_port(detail) {
        return String::new();
    }
    if lower.contains("adb reverse 失败") || detail.contains("adb reverse 失败") {
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
    if snap.phase == "已连接" {
        return "平板已连上".into();
    }
    if looks_technical(detail) {
        return String::new();
    }
    detail.to_string()
}

fn human_last_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if raw.contains("找不到 adb") || lower.contains("adb.exe") {
        return "未检测到 USB 驱动。请安装平台工具，或换一根能传数据的线。".into();
    }
    if raw.contains("没有可用显示器") {
        return "没有可用显示器".into();
    }
    if looks_like_bind_or_port(raw) {
        return "无法开始共享，请稍后重试".into();
    }
    raw.lines().next().unwrap_or(raw).to_string()
}

fn looks_like_bind_or_port(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("17400")
        || lower.contains("0.0.0.0")
        || lower.contains("127.0.0.1")
        || lower.contains("connection refused")
        || text.contains("绑定")
}

fn looks_technical(text: &str) -> bool {
    looks_like_bind_or_port(text) || text.contains("adb reverse") || text.contains("tcp:")
}
