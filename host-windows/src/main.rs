#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

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
        Ok(file) => BoxMakeWriter::new(std::sync::Mutex::new(file)),
        // No console in release GUI builds — fall back to a sink rather than stderr.
        Err(_) => BoxMakeWriter::new(std::io::sink),
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
            .with_inner_size([560.0, 1024.0])
            .with_min_inner_size([540.0, 920.0])
            .with_decorations(false)
            .with_transparent(false)
            .with_resizable(true)
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
    /// Background `adb devices` result — never block the UI thread on adb.
    pending_devices: Arc<Mutex<Option<Result<Vec<adb::AdbDevice>, String>>>>,
    device_refresh_inflight: bool,
    /// One-shot notice after APK install attempt.
    pending_notice: Arc<Mutex<Option<(Tone, String)>>>,
    install_inflight: bool,
    apk_available: bool,
    notice: Option<(Tone, String)>,
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
            last_poll: Instant::now() - Duration::from_secs(10),
            pending_devices: Arc::new(Mutex::new(None)),
            device_refresh_inflight: false,
            pending_notice: Arc::new(Mutex::new(None)),
            install_inflight: false,
            apk_available: adb::find_bundled_apk().is_some(),
            notice: None,
        };
        app.request_device_refresh();
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

    fn apply_pending_devices(&mut self) {
        let result = match self.pending_devices.lock() {
            Ok(mut slot) => slot.take(),
            Err(_) => None,
        };
        if let Some(result) = result {
            self.device_refresh_inflight = false;
            match result {
                Ok(list) => {
                    self.devices = list;
                    self.select_usb_device();
                    if self.last_error.contains("adb") || self.last_error.contains("找不到") {
                        self.last_error.clear();
                    }
                }
                Err(err) => self.last_error = err,
            }
        }

        let install_notice = if let Ok(mut slot) = self.pending_notice.lock() {
            slot.take()
        } else {
            None
        };
        if let Some(notice) = install_notice {
            self.install_inflight = false;
            self.notice = Some(notice);
            // Re-probe so the hint flips once the package appears.
            self.request_device_refresh();
        }
        // Drop install notices once the package is confirmed on the device.
        if !self.client_app_missing() && !self.install_inflight {
            self.notice = None;
        }
        self.apk_available = adb::find_bundled_apk().is_some();
    }

    fn primary_ready_device(&self) -> Option<&adb::AdbDevice> {
        self.devices
            .get(self.settings.selected_device)
            .filter(|d| d.state == "device")
            .or_else(|| self.devices.iter().find(|d| d.state == "device"))
    }

    fn client_app_missing(&self) -> bool {
        self.primary_ready_device()
            .and_then(|d| d.client_installed)
            .is_some_and(|installed| !installed)
    }

    fn request_install_client(&mut self) {
        if self.install_inflight {
            return;
        }
        let Some(apk) = adb::find_bundled_apk() else {
            self.notice = Some((
                Tone::Warn,
                "电脑这边还没有 APK 文件。请把 Lighting.apk 放到程序同目录后再试".into(),
            ));
            return;
        };
        let Some(serial) = self.primary_ready_device().map(|d| d.serial.clone()) else {
            self.notice = Some((Tone::Warn, "请先用数据线连接平板并打开 USB 调试".into()));
            return;
        };
        let adb = match adb::find_adb() {
            Ok(p) => p,
            Err(err) => {
                self.notice = Some((Tone::Bad, format!("{err:#}")));
                return;
            }
        };
        self.install_inflight = true;
        self.notice = Some((Tone::Info, ui_text::client_app_installing_hint()));
        let pending = self.pending_notice.clone();
        self.rt.spawn(async move {
            let notice = match adb::install_apk(&adb, &serial, &apk).await {
                Ok(()) => (Tone::Ok, ui_text::client_app_installed_ok()),
                Err(err) => (
                    Tone::Bad,
                    format!("安装失败：{err:#}。也可把 APK 拷到平板里手动安装"),
                ),
            };
            if let Ok(mut slot) = pending.lock() {
                *slot = Some(notice);
            }
        });
    }

    fn request_device_refresh(&mut self) {
        if self.device_refresh_inflight {
            return;
        }
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
        self.device_refresh_inflight = true;
        let pending = self.pending_devices.clone();
        self.rt.spawn(async move {
            let result = adb::list_devices(&adb)
                .await
                .map_err(|err| format!("{err:#}"));
            if let Ok(mut slot) = pending.lock() {
                *slot = Some(result);
            }
        });
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
        self.request_device_refresh();
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
        if let Some((tone, text)) = &self.notice {
            return (text.clone(), *tone);
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
        if ready >= 1 && self.client_app_missing() {
            return ui_text::client_app_missing_hint(self.apk_available);
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
            client_app_missing: self.client_app_missing(),
            can_install_apk: self.apk_available && !self.install_inflight,
            install_inflight: self.install_inflight,
            displays: self
                .displays
                .iter()
                .enumerate()
                .map(|(i, d)| ui_text::display_choice_label(i, d.primary, d.width, d.height))
                .collect(),
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
        self.apply_pending_devices();
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
                    self.request_device_refresh();
                }
                Action::InstallClient => self.request_install_client(),
                Action::TouchRelayChanged => self
                    .controls
                    .touch
                    .store(self.settings.touch_relay, Ordering::Relaxed),
            }
        }
    }
}
