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
use lighting_host::ui_text::{self, Tone};
use lighting_host::view::{self, Action, ResCap, Settings};
use lighting_host::theme;
use session::{live_transport, SessionRequest, SessionStatus};
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
            .with_inner_size([560.0, 860.0])
            .with_min_inner_size([470.0, 560.0])
            .with_title("Lighting 副屏"),
        ..Default::default()
    };
    eframe::run_native(
        "Lighting",
        native,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(LightingApp::new()))
        }),
    )
}

struct LightingApp {
    rt: tokio::runtime::Runtime,
    displays: Vec<displays::DisplayInfo>,
    devices: Vec<adb::AdbDevice>,
    settings: Settings,
    status: Arc<Mutex<SessionStatus>>,
    controls: Arc<session::Controls>,
    stop: Arc<AtomicBool>,
    running: bool,
    last_error: String,
    adb_path: String,
    last_poll: Instant,
}

impl LightingApp {
    fn new() -> Self {
        let rt = tokio::runtime::Runtime::new().expect("tokio");
        let displays = displays::list_displays().unwrap_or_default();
        let settings = Settings {
            selected_display: displays.iter().position(|d| !d.primary).unwrap_or(0),
            bind_port: protocol::PORT,
            ..Default::default()
        };
        let mut app = Self {
            rt,
            displays,
            devices: Vec::new(),
            settings,
            status: Arc::new(Mutex::new(SessionStatus::default())),
            controls: Arc::new(session::Controls::default()),
            stop: Arc::new(AtomicBool::new(false)),
            running: false,
            last_error: String::new(),
            adb_path: String::new(),
            last_poll: Instant::now(),
        };
        app.refresh_devices();
        app
    }

    fn refresh_displays(&mut self) {
        match displays::list_displays() {
            Ok(list) => {
                self.displays = list;
                if self.settings.selected_display >= self.displays.len() {
                    self.settings.selected_display = 0;
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
            self.settings.selected_device = ready[0];
        } else if self.settings.selected_device >= self.devices.len() {
            self.settings.selected_device = 0;
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
        let quality = (self.settings.quality_pct.clamp(40, 100) as f32) / 100.0;
        let (match_device, scale, max_width, max_height) = match self.settings.res_cap {
            ResCap::Device => (true, quality, 3840, 2560),
            ResCap::Fhd => (false, 1.0, scaled(1920, quality), scaled(1080, quality)),
            ResCap::Uhd2k => (false, 1.0, scaled(2560, quality), scaled(1440, quality)),
        };
        let serial = self
            .devices
            .get(self.settings.selected_device)
            .filter(|d| d.state == "device")
            .map(|d| d.serial.clone())
            .or_else(|| {
                self.devices
                    .iter()
                    .find(|d| d.state == "device")
                    .map(|d| d.serial.clone())
            });
        let bind_host = self.settings.bind_host.trim();
        let bind_host = if bind_host.is_empty() {
            "0.0.0.0"
        } else {
            bind_host
        };
        let bind_port = if self.settings.bind_port == 0 {
            protocol::PORT
        } else {
            self.settings.bind_port
        };
        let req = SessionRequest {
            display_index: self.settings.selected_display,
            device_serial: serial,
            bind: format!("{bind_host}:{bind_port}"),
            prefer_hevc: self.settings.prefer_hevc,
            bitrate_kbps: self.settings.bitrate_kbps,
            fps: self.settings.fps,
            max_width,
            max_height,
            match_device,
            scale,
            send_audio: self.settings.send_audio,
        };
        self.stop.store(false, Ordering::Relaxed);
        self.controls
            .touch
            .store(self.settings.touch_relay, Ordering::Relaxed);
        if let Ok(mut s) = self.status.lock() {
            *s = SessionStatus {
                running: true,
                phase: "启动中".into(),
                ..Default::default()
            };
        }
        let status = self.status.clone();
        let stop = self.stop.clone();
        let controls = self.controls.clone();
        self.rt
            .spawn(session::run_session(req, status, stop, controls));
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

    fn ready_devices(&self) -> usize {
        self.devices.iter().filter(|d| d.state == "device").count()
    }

    /// USB readiness in beginner wording: the live session transport wins, then
    /// the adb probe, so the hint always names the next thing to try.
    fn usb_hint(&self, snap: &SessionStatus) -> (String, Tone) {
        if let Some(raw) = live_transport(snap.running, &snap.transport) {
            return ui_text::humanize_transport(raw);
        }
        let ready = self.ready_devices();
        let pending = self
            .devices
            .iter()
            .any(|d| d.state == "unauthorized" || d.state == "offline");
        if self.adb_path.is_empty() {
            return (
                "未检测到 USB 驱动。请换数据线，或在高级里用局域网连接".into(),
                Tone::Warn,
            );
        }
        if pending && ready == 0 {
            return ("请打开 USB 调试并点允许".into(), Tone::Warn);
        }
        if ready == 1 {
            let name = self
                .devices
                .iter()
                .find(|d| d.state == "device")
                .map(|d| d.serial.as_str())
                .unwrap_or("");
            return (format!("已找到设备，将自动连接 · {name}"), Tone::Ok);
        }
        if ready > 1 {
            return ("检测到多台设备，请选择一台".into(), Tone::Info);
        }
        (
            "未检测到设备。请检查数据线是否支持传数据".into(),
            Tone::Warn,
        )
    }

    fn snapshot(&self, status: &SessionStatus) -> view::Snapshot {
        let (usb_hint, usb_tone) = self.usb_hint(status);
        view::Snapshot {
            running: status.running,
            phase: status.phase.clone(),
            detail: status.detail.clone(),
            transport: status.transport.clone(),
            client_name: status.client_name.clone(),
            client_addr: status.client_addr.clone(),
            codec: status.codec.clone(),
            frames: status.frames,
            bitrate_kbps: status.bitrate_kbps,
            latency_ms: status.latency_ms,
            loss_permille: status.loss_permille,
            bytes_sent: status.bytes_sent,
            connected_secs: status.connected_secs,
            usb_hint,
            usb_tone,
            displays: self.displays.iter().map(|d| d.label()).collect(),
            devices: self.devices.iter().map(|d| d.label()).collect(),
            multi_device: self.ready_devices() > 1,
            adb_path: self.adb_path.clone(),
            last_error: self.last_error.clone(),
        }
    }
}

fn scaled(value: u32, quality: f32) -> u32 {
    (((value as f32) * quality) as u32 & !1).max(320)
}

impl eframe::App for LightingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(200));
        self.maybe_poll_devices();
        let status = self
            .status
            .lock()
            .ok()
            .map(|s| s.clone())
            .unwrap_or_default();
        if self.running && !status.running && status.phase == "错误" {
            self.running = false;
        }
        if status.phase == "已停止" {
            self.running = false;
        }

        let snapshot = self.snapshot(&status);
        for action in view::render(ctx, &snapshot, &mut self.settings) {
            match action {
                Action::Start => self.start(),
                Action::Stop => self.stop_session(),
                Action::Refresh => {
                    self.refresh_displays();
                    self.refresh_devices();
                }
                Action::TouchRelayChanged => self
                    .controls
                    .touch
                    .store(self.settings.touch_relay, Ordering::Relaxed),
            }
        }
    }
}
