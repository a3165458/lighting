//! Shared host control core used by egui UI and the localhost IPC server.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lighting_host::host_ipc::{
    DeviceDto, DisplayDto, HostSettingsDto, HostStateDto, SettingsPatchDto,
};
use lighting_host::ui_text::{self, Tone};
use lighting_host::view::{ResCap, Settings, ShareMode, Snapshot};

use crate::adb;
use crate::displays;
use crate::protocol;
use crate::session::{self, live_transport, SessionRequest, SessionStatus};

pub struct HostService {
    inner: Arc<Mutex<HostInner>>,
    rt: Arc<tokio::runtime::Runtime>,
}

struct HostInner {
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
    pending_devices: Arc<Mutex<Option<Result<Vec<adb::AdbDevice>, String>>>>,
    device_refresh_inflight: bool,
    pending_notice: Arc<Mutex<Option<(Tone, String)>>>,
    install_inflight: bool,
    apk_available: bool,
    notice: Option<(Tone, String)>,
}

impl HostService {
    pub fn new() -> Self {
        let rt = Arc::new(tokio::runtime::Runtime::new().expect("tokio"));
        let displays = displays::list_displays().unwrap_or_default();
        let settings = Settings {
            selected_display: displays.iter().position(|d| !d.primary).unwrap_or(0),
            bind_port: protocol::PORT,
            ..Default::default()
        };
        let mut inner = HostInner {
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
        Self::request_device_refresh_locked(&rt, &mut inner);
        Self {
            inner: Arc::new(Mutex::new(inner)),
            rt,
        }
    }

    pub fn runtime(&self) -> Arc<tokio::runtime::Runtime> {
        self.rt.clone()
    }

    pub fn clone_handle(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            rt: self.rt.clone(),
        }
    }

    /// Background maintenance: apply adb results, refresh devices while idle.
    pub fn tick(&self) {
        let mut g = self.inner.lock().expect("host lock");
        Self::apply_pending_devices_locked(&self.rt, &mut g);
        if !g.running && g.last_poll.elapsed() >= Duration::from_secs(2) {
            g.last_poll = Instant::now();
            Self::request_device_refresh_locked(&self.rt, &mut g);
        }
    }

    pub fn refresh(&self) {
        let mut g = self.inner.lock().expect("host lock");
        match displays::list_displays() {
            Ok(list) => {
                g.displays = list;
                if g.settings.selected_display >= g.displays.len() {
                    g.settings.selected_display = 0;
                }
            }
            Err(err) => g.last_error = format!("{err:#}"),
        }
        Self::request_device_refresh_locked(&self.rt, &mut g);
    }

    pub fn start_share(&self) -> Result<(), String> {
        let mode = {
            let g = self.inner.lock().expect("host lock");
            if g.running {
                return Ok(());
            }
            g.settings.share_mode
        };

        if matches!(mode, ShareMode::Extend | ShareMode::External) {
            {
                let mut g = self.inner.lock().expect("host lock");
                g.notice = Some((
                    Tone::Info,
                    "正在准备扩展屏…首次可能弹出管理员确认".into(),
                ));
                g.last_error.clear();
            }
            if let Err(err) = displays::ensure_secondary_display(mode) {
                let msg = format!("{err:#}");
                let mut g = self.inner.lock().expect("host lock");
                g.last_error = msg.clone();
                g.notice = Some((Tone::Warn, msg.clone()));
                return Err(msg);
            }
        } else if let Err(err) = displays::apply_project_mode(mode) {
            tracing::warn!("DisplaySwitch failed ({err:#}); continuing with current layout");
        }

        let mut g = self.inner.lock().expect("host lock");
        match displays::list_displays() {
            Ok(list) => g.displays = list,
            Err(err) => {
                g.last_error = format!("{err:#}");
                return Err(g.last_error.clone());
            }
        }
        if g.displays.is_empty() {
            g.last_error = "没有可用显示器".into();
            return Err(g.last_error.clone());
        }
        if matches!(mode, ShareMode::Extend | ShareMode::External) && !displays::has_secondary(&g.displays)
        {
            let msg = "扩展屏尚未就绪，请重试「开始共享」，或检查是否取消了管理员确认。".to_string();
            g.last_error = msg.clone();
            g.notice = Some((Tone::Warn, msg.clone()));
            return Err(msg);
        }
        if let Some(idx) = displays::pick_display_index(&g.displays, mode) {
            g.settings.selected_display = idx;
        }

        let quality = (g.settings.quality_pct.clamp(40, 100) as f32) / 100.0;
        let (match_device, scale, max_width, max_height) = match g.settings.res_cap {
            ResCap::Device => (true, quality, 3840, 2560),
            ResCap::Fhd => (false, 1.0, scaled(1920, quality), scaled(1080, quality)),
            ResCap::Uhd2k => (false, 1.0, scaled(2560, quality), scaled(1440, quality)),
            ResCap::Uhd4k => (false, 1.0, scaled(3840, quality), scaled(2160, quality)),
        };
        let serial = g
            .devices
            .get(g.settings.selected_device)
            .filter(|d| d.state == "device")
            .map(|d| d.serial.clone())
            .or_else(|| {
                g.devices
                    .iter()
                    .find(|d| d.state == "device")
                    .map(|d| d.serial.clone())
            });
        let bind_host = g.settings.bind_host.trim();
        let bind_host = if bind_host.is_empty() {
            "0.0.0.0"
        } else {
            bind_host
        };
        let bind_port = if g.settings.bind_port == 0 {
            protocol::PORT
        } else {
            g.settings.bind_port
        };
        let req = SessionRequest {
            display_index: g.settings.selected_display,
            device_serial: serial,
            bind: format!("{bind_host}:{bind_port}"),
            prefer_hevc: g.settings.prefer_hevc,
            bitrate_kbps: g.settings.bitrate_kbps,
            fps: g.settings.fps,
            max_width,
            max_height,
            match_device,
            scale,
            send_audio: g.settings.send_audio,
        };
        g.stop.store(false, Ordering::Relaxed);
        g.controls
            .touch
            .store(g.settings.touch_relay, Ordering::Relaxed);
        if let Ok(mut s) = g.status.lock() {
            *s = SessionStatus {
                running: true,
                phase: "启动中".into(),
                ..Default::default()
            };
        };
        let status = g.status.clone();
        let stop = g.stop.clone();
        let controls = g.controls.clone();
        self.rt
            .spawn(session::run_session(req, status, stop, controls));
        g.running = true;
        Ok(())
    }

    pub fn stop_share(&self) {
        let mut g = self.inner.lock().expect("host lock");
        g.stop.store(true, Ordering::Relaxed);
        g.running = false;
        if let Ok(mut s) = g.status.lock() {
            s.transport.clear();
            s.bitrate_kbps = 0;
            s.frames = 0;
            s.detail.clear();
        };
    }

    pub fn patch_settings(&self, patch: SettingsPatchDto) {
        let mut g = self.inner.lock().expect("host lock");
        if let Some(v) = patch.selected_display {
            if v < g.displays.len() {
                g.settings.selected_display = v;
            }
        }
        if let Some(v) = patch.selected_device {
            if v < g.devices.len() {
                g.settings.selected_device = v;
            }
        }
        if let Some(v) = patch.quality_pct {
            g.settings.quality_pct = v.clamp(40, 100);
        }
        if let Some(v) = patch.fps {
            g.settings.fps = v.clamp(15, 120);
        }
        if let Some(v) = patch.bitrate_kbps {
            g.settings.bitrate_kbps = v.clamp(1_000, 80_000);
        }
        if let Some(v) = patch.send_audio {
            g.settings.send_audio = v;
        }
        if let Some(v) = patch.prefer_hevc {
            g.settings.prefer_hevc = v;
        }
        if let Some(v) = patch.share_mode {
            if let Some(mode) = ShareMode::from_wire(&v) {
                g.settings.share_mode = mode;
                if let Some(idx) = displays::pick_display_index(&g.displays, mode) {
                    g.settings.selected_display = idx;
                }
            }
        }
        if let Some(v) = patch.res_cap {
            if let Some(cap) = ResCap::from_wire(&v) {
                g.settings.res_cap = cap;
            }
        }
        if let Some(v) = patch.touch_relay {
            g.settings.touch_relay = v;
            g.controls.touch.store(v, Ordering::Relaxed);
        }
        if let Some(v) = patch.keyboard_relay {
            g.settings.keyboard_relay = v;
        }
        if let Some(v) = patch.bind_host {
            g.settings.bind_host = v;
        }
        if let Some(v) = patch.bind_port {
            g.settings.bind_port = v;
        }
    }

    pub fn install_client(&self) -> Result<(), String> {
        let mut g = self.inner.lock().expect("host lock");
        if g.install_inflight {
            return Ok(());
        }
        let Some(apk) = adb::find_bundled_apk() else {
            let msg = "电脑这边还没有 APK 文件。请把 Lighting.apk 放到程序同目录后再试".to_string();
            g.notice = Some((Tone::Warn, msg.clone()));
            return Err(msg);
        };
        let Some(serial) = g
            .devices
            .get(g.settings.selected_device)
            .filter(|d| d.state == "device")
            .map(|d| d.serial.clone())
            .or_else(|| {
                g.devices
                    .iter()
                    .find(|d| d.state == "device")
                    .map(|d| d.serial.clone())
            })
        else {
            let msg = "请先用数据线连接平板并打开 USB 调试".to_string();
            g.notice = Some((Tone::Warn, msg.clone()));
            return Err(msg);
        };
        let adb = match adb::find_adb() {
            Ok(p) => p,
            Err(err) => {
                let msg = format!("{err:#}");
                g.notice = Some((Tone::Bad, msg.clone()));
                return Err(msg);
            }
        };
        g.install_inflight = true;
        g.notice = Some((Tone::Info, ui_text::client_app_installing_hint()));
        let pending = g.pending_notice.clone();
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
        Ok(())
    }

    pub fn state(&self) -> HostStateDto {
        let g = self.inner.lock().expect("host lock");
        let status = g.status.lock().ok().map(|s| s.clone()).unwrap_or_default();
        let (usb_hint, usb_tone) = usb_hint_locked(&g, &status);
        let ready = g.devices.iter().filter(|d| d.state == "device").count();
        let selected = g
            .devices
            .get(g.settings.selected_device)
            .filter(|d| d.state == "device")
            .or_else(|| g.devices.iter().find(|d| d.state == "device"));
        let client_app_missing = selected
            .and_then(|d| d.client_installed)
            .is_some_and(|installed| !installed);
        let client_app_version = selected
            .and_then(|d| d.client_version.clone())
            .unwrap_or_default();

        HostStateDto {
            connected: true,
            sharing: status.running || g.running,
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
            usb_tone: tone_str(usb_tone).into(),
            device_detected: ready >= 1,
            client_app_missing,
            client_app_version,
            can_install_apk: g.apk_available && !g.install_inflight,
            install_inflight: g.install_inflight,
            multi_device: ready > 1,
            displays: g
                .displays
                .iter()
                .enumerate()
                .map(|(i, d)| DisplayDto {
                    id: i.to_string(),
                    label: d.label(),
                    primary: d.primary,
                    width: d.width,
                    height: d.height,
                    virtual_display: d.is_virtual,
                })
                .collect(),
            devices: g
                .devices
                .iter()
                .enumerate()
                .map(|(i, d)| DeviceDto {
                    id: i.to_string(),
                    label: d.label(),
                    serial: d.serial.clone(),
                    state: d.state.clone(),
                    client_installed: d.client_installed,
                    client_version: d.client_version.clone(),
                })
                .collect(),
            settings: HostSettingsDto {
                selected_display: g.settings.selected_display,
                selected_device: g.settings.selected_device,
                share_mode: g.settings.share_mode.as_wire().into(),
                quality_pct: g.settings.quality_pct,
                fps: g.settings.fps,
                bitrate_kbps: g.settings.bitrate_kbps,
                send_audio: g.settings.send_audio,
                prefer_hevc: g.settings.prefer_hevc,
                res_cap: g.settings.res_cap.as_wire().into(),
                touch_relay: g.settings.touch_relay,
                keyboard_relay: g.settings.keyboard_relay,
                bind_host: g.settings.bind_host.clone(),
                bind_port: g.settings.bind_port,
            },
            last_error: g.last_error.clone(),
            host_version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    /// egui bridge: mutate settings + render snapshot pieces.
    pub fn with_ui<R>(&self, f: impl FnOnce(&mut Settings, Snapshot) -> R) -> R {
        let mut g = self.inner.lock().expect("host lock");
        Self::apply_pending_devices_locked(&self.rt, &mut g);
        let status = g.status.lock().ok().map(|s| s.clone()).unwrap_or_default();
        let snap = ui_snapshot_locked(&g, &status);
        f(&mut g.settings, snap)
    }

    pub fn set_running_flag_from_status(&self) {
        let mut g = self.inner.lock().expect("host lock");
        let running = g.status.lock().ok().map(|s| s.running).unwrap_or(false);
        g.running = running;
    }

    fn apply_pending_devices_locked(rt: &tokio::runtime::Runtime, g: &mut HostInner) {
        let result = match g.pending_devices.lock() {
            Ok(mut slot) => slot.take(),
            Err(_) => None,
        };
        if let Some(result) = result {
            g.device_refresh_inflight = false;
            match result {
                Ok(list) => {
                    g.devices = list;
                    select_usb_device(g);
                    if g.last_error.contains("adb") || g.last_error.contains("找不到") {
                        g.last_error.clear();
                    }
                }
                Err(err) => g.last_error = err,
            }
        }

        let install_notice = match g.pending_notice.lock() {
            Ok(mut slot) => slot.take(),
            Err(_) => None,
        };
        if let Some(notice) = install_notice {
            g.install_inflight = false;
            g.notice = Some(notice);
            Self::request_device_refresh_locked(rt, g);
        }
        let missing = g
            .devices
            .iter()
            .find(|d| d.state == "device")
            .and_then(|d| d.client_installed)
            .is_some_and(|installed| !installed);
        if !missing && !g.install_inflight {
            g.notice = None;
        }
        g.apk_available = adb::find_bundled_apk().is_some();
    }

    fn request_device_refresh_locked(rt: &tokio::runtime::Runtime, g: &mut HostInner) {
        if g.device_refresh_inflight {
            return;
        }
        let adb = match adb::find_adb() {
            Ok(p) => {
                g.adb_path = p.display().to_string();
                p
            }
            Err(err) => {
                g.adb_path.clear();
                g.last_error = format!("{err:#}");
                g.devices.clear();
                return;
            }
        };
        g.device_refresh_inflight = true;
        let pending = g.pending_devices.clone();
        rt.spawn(async move {
            let result = adb::list_devices(&adb)
                .await
                .map_err(|err| format!("{err:#}"));
            if let Ok(mut slot) = pending.lock() {
                *slot = Some(result);
            }
        });
    }
}

fn select_usb_device(g: &mut HostInner) {
    let ready: Vec<usize> = g
        .devices
        .iter()
        .enumerate()
        .filter(|(_, d)| d.state == "device")
        .map(|(i, _)| i)
        .collect();
    if ready.len() == 1 {
        g.settings.selected_device = ready[0];
    } else if g.settings.selected_device >= g.devices.len() {
        g.settings.selected_device = 0;
    }
}

fn tone_str(tone: Tone) -> &'static str {
    match tone {
        Tone::Ok => "ok",
        Tone::Warn => "warn",
        Tone::Bad => "bad",
        Tone::Info => "info",
        Tone::Muted => "muted",
    }
}

fn usb_hint_locked(g: &HostInner, snap: &SessionStatus) -> (String, Tone) {
    if let Some(raw) = live_transport(snap.running, &snap.transport) {
        return ui_text::humanize_transport(raw);
    }
    if let Some((tone, text)) = &g.notice {
        return (text.clone(), *tone);
    }
    let ready = g.devices.iter().filter(|d| d.state == "device").count();
    let pending = g
        .devices
        .iter()
        .any(|d| d.state == "unauthorized" || d.state == "offline");
    if g.adb_path.is_empty() {
        return (
            "未检测到 USB 驱动。请换数据线，或在高级里用局域网连接".into(),
            Tone::Warn,
        );
    }
    if pending && ready == 0 {
        return ("请打开 USB 调试并点允许".into(), Tone::Warn);
    }
    let missing = g
        .devices
        .iter()
        .find(|d| d.state == "device")
        .and_then(|d| d.client_installed)
        .is_some_and(|installed| !installed);
    if ready >= 1 && missing {
        return ui_text::client_app_missing_hint(g.apk_available);
    }
    if ready == 1 {
        let name = g
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

fn ui_snapshot_locked(g: &HostInner, status: &SessionStatus) -> lighting_host::view::Snapshot {
    let (usb_hint, usb_tone) = usb_hint_locked(g, status);
    let ready = g.devices.iter().filter(|d| d.state == "device").count();
    let client_app_missing = g
        .devices
        .iter()
        .find(|d| d.state == "device")
        .and_then(|d| d.client_installed)
        .is_some_and(|installed| !installed);
    lighting_host::view::Snapshot {
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
        client_app_missing,
        can_install_apk: g.apk_available && !g.install_inflight,
        install_inflight: g.install_inflight,
        displays: g.displays.iter().map(|d| d.label()).collect(),
        devices: g.devices.iter().map(|d| d.label()).collect(),
        multi_device: ready > 1,
        adb_path: g.adb_path.clone(),
        last_error: g.last_error.clone(),
    }
}

fn scaled(value: u32, quality: f32) -> u32 {
    (((value as f32) * quality) as u32 & !1).max(320)
}
