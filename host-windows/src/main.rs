mod adb;
mod audio;
mod displays;
mod encoder;
mod input;
mod protocol;
mod session;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use session::{SessionRequest, SessionStatus};
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
        let selected_display = displays
            .iter()
            .position(|d| !d.primary)
            .unwrap_or(0);
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
                if self.selected_device >= self.devices.len() {
                    self.selected_device = 0;
                }
                self.last_error.clear();
            }
            Err(err) => self.last_error = format!("{err:#}"),
        }
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
            .map(|d| d.serial.clone());
        let req = SessionRequest {
            display_index: self.selected_display,
            device_serial: serial,
            bind: format!("0.0.0.0:{}", protocol::PORT),
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
    }
}

impl eframe::App for LightingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let snap = self.status.lock().ok().map(|s| s.clone()).unwrap_or_default();
        if self.running && !snap.running && snap.phase == "错误" {
            self.running = false;
        }
        if snap.phase == "已停止" {
            self.running = false;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Lighting 副屏");
            ui.label("把 Android 平板/手机当成电脑的扩展屏。USB 数据线即可（不限 Type-C）。");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui.button("刷新显示器").clicked() {
                    self.refresh_displays();
                }
                if ui.button("刷新设备").clicked() {
                    self.refresh_devices();
                }
            });

            ui.add_space(6.0);
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

            ui.add_space(6.0);
            ui.label("Android 设备（需开启 USB 调试）：");
            if !self.adb_path.is_empty() {
                ui.weak(format!("adb：{}", self.adb_path));
            }
            if self.devices.is_empty() {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 180, 80),
                    "未找到设备。可先不插线，平板填写电脑 IP 做局域网测试。",
                );
            } else {
                egui::ComboBox::from_id_salt("device")
                    .selected_text(
                        self.devices
                            .get(self.selected_device)
                            .map(|d| d.label())
                            .unwrap_or_default(),
                    )
                    .show_ui(ui, |ui| {
                        for (i, d) in self.devices.iter().enumerate() {
                            ui.selectable_value(&mut self.selected_device, i, d.label());
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
            ui.horizontal(|ui| {
                let start = ui.add_enabled(!snap.running, egui::Button::new("开始共享"));
                if start.clicked() {
                    self.start();
                }
                let stop = ui.add_enabled(snap.running, egui::Button::new("停止"));
                if stop.clicked() {
                    self.stop_session();
                }
            });

            ui.add_space(10.0);
            ui.separator();
            ui.label(format!("状态：{}", if snap.phase.is_empty() { "空闲" } else { &snap.phase }));
            if !snap.detail.is_empty() {
                ui.label(&snap.detail);
            }
            ui.label(format!("已发送帧：{}", snap.frames));
            if !self.last_error.is_empty() {
                ui.colored_label(egui::Color32::from_rgb(220, 80, 80), &self.last_error);
            }

            ui.add_space(12.0);
            ui.weak("扩展屏：winget install VirtualDrivers.Virtual-Display-Driver ，然后在 Windows 显示设置里设为「扩展」并选这块虚拟屏。");
            ui.weak("USB：电脑执行 adb reverse 后，平板连接 127.0.0.1:17400。平板触控：单击/拖动、长按右键、双指滚动。");
        });
    }
}
