use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};

use crate::adb;
use crate::displays::{self, DisplayInfo};
use crate::encoder::{self, EncodeSettings, EncodedPacket};
use crate::input;
use crate::protocol::{self, Hello, StreamConfig, FLAG_CODEC_CONFIG, FLAG_KEYFRAME};
use lighting_host::annexb;
use lighting_host::session_policy;

#[derive(Clone)]
pub struct SessionRequest {
    pub display_index: usize,
    pub device_serial: Option<String>,
    pub bind: String,
    pub prefer_hevc: bool,
    pub bitrate_kbps: u32,
    pub fps: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub match_device: bool,
    pub scale: f32,
    pub send_audio: bool,
    pub share_mode: lighting_host::view::ShareMode,
}

#[derive(Clone, Default)]
pub struct SessionStatus {
    pub running: bool,
    pub phase: String,
    pub detail: String,
    pub frames: u64,
    pub bitrate_kbps: u32,
    pub transport: String,
    pub client_name: String,
    pub client_addr: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Heartbeat round-trip, smoothed. 0 until the client answers once.
    pub latency_ms: u32,
    /// TCP repairs loss below us, so this stays 0; kept for a future UDP path.
    pub loss_permille: u32,
    pub bytes_sent: u64,
    pub connected_secs: u64,
}

/// Interaction switches the user can flip while the share is live.
pub struct Controls {
    pub touch: AtomicBool,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            touch: AtomicBool::new(true),
        }
    }
}

pub async fn run_session(
    req: SessionRequest,
    status: Arc<Mutex<SessionStatus>>,
    stop: Arc<AtomicBool>,
    controls: Arc<Controls>,
) {
    let result = run_session_inner(req, status.clone(), stop, controls).await;
    if let Err(err) = result {
        tracing::error!("{err:#}");
        if let Ok(mut s) = status.lock() {
            s.running = false;
            s.phase = "错误".into();
            s.detail = format!("{err:#}");
            clear_share_metrics(&mut s);
        }
    } else if let Ok(mut s) = status.lock() {
        s.running = false;
        if s.phase != "错误" {
            s.phase = "已停止".into();
        }
        clear_share_metrics(&mut s);
    }
}

fn set_status(status: &Arc<Mutex<SessionStatus>>, phase: &str, detail: impl Into<String>) {
    if let Ok(mut s) = status.lock() {
        s.running = true;
        s.phase = phase.into();
        s.detail = detail.into();
    }
}

fn set_transport(status: &Arc<Mutex<SessionStatus>>, transport: impl Into<String>) {
    if let Ok(mut s) = status.lock() {
        s.transport = transport.into();
    }
}

fn set_bitrate(status: &Arc<Mutex<SessionStatus>>, bitrate_kbps: u32) {
    if let Ok(mut s) = status.lock() {
        s.bitrate_kbps = bitrate_kbps;
    }
}

const HEADER_BYTES: usize = 12;

fn add_wire_bytes(status: &Arc<Mutex<SessionStatus>>, bytes: usize) {
    if let Ok(mut s) = status.lock() {
        s.bytes_sent += bytes as u64;
    }
}

pub fn live_transport(running: bool, transport: &str) -> Option<&str> {
    if running && !transport.is_empty() {
        Some(transport)
    } else {
        None
    }
}

fn clear_share_metrics(status: &mut SessionStatus) {
    status.transport.clear();
    status.bitrate_kbps = 0;
    status.frames = 0;
    if status.phase != "错误" {
        status.detail.clear();
    }
    clear_peer_metrics(status);
}

/// Reset everything tied to one tablet so a stale device name or latency never
/// outlives its session.
fn clear_peer_metrics(status: &mut SessionStatus) {
    status.client_name.clear();
    status.client_addr.clear();
    status.codec.clear();
    status.width = 0;
    status.height = 0;
    status.fps = 0;
    status.latency_ms = 0;
    status.loss_permille = 0;
    status.bytes_sent = 0;
    status.connected_secs = 0;
}

async fn run_session_inner(
    req: SessionRequest,
    status: Arc<Mutex<SessionStatus>>,
    stop: Arc<AtomicBool>,
    controls: Arc<Controls>,
) -> Result<()> {
    let mut req = req;
    let desktop = if req.share_mode.uses_virtual_display() {
        displays::DesktopRestoreGuard::capture()
    } else {
        displays::DesktopRestoreGuard { primary: None }
    };
    let preserve = desktop.primary.clone();
    if req.share_mode.uses_virtual_display() {
        set_status(&status, "准备虚拟屏", "正在检查并启用虚拟显示器…");
        let mode = req.share_mode;
        let status_prog = status.clone();
        let preserve_for_drv = preserve.clone();
        let ensure = tokio::task::spawn_blocking(move || {
            displays::ensure_secondary_display_with_progress(
                mode,
                |step| {
                    if let Ok(mut s) = status_prog.lock() {
                        s.running = true;
                        s.phase = "准备虚拟屏".into();
                        s.detail = step.to_string();
                    }
                },
                preserve_for_drv.as_ref(),
            )
        })
        .await;
        let ensure_result = match ensure {
            Ok(inner) => inner,
            Err(err) => Err(anyhow::anyhow!("启用虚拟屏任务中断: {err:#}")),
        };
        let (ensure_ok, ensure_err) = match &ensure_result {
            Ok(()) => (true, String::new()),
            Err(err) => (false, format!("{err:#}")),
        };
        let list = displays::list_displays().unwrap_or_default();
        match lighting_host::share_flow::decide_after_virtual_prepare(
            req.share_mode,
            ensure_ok,
            &ensure_err,
            displays::has_secondary(&list),
            displays::has_virtual_display(&list),
        ) {
            lighting_host::share_flow::VirtualPrepareOutcome::Ready => {
                if let Some(idx) = displays::pick_display_index(&list, req.share_mode) {
                    req.display_index = idx;
                }
                set_status(&status, "准备虚拟屏", "虚拟屏已就绪，开始等待平板…");
            }
            lighting_host::share_flow::VirtualPrepareOutcome::Abort { reason } => {
                anyhow::bail!(
                    "{}",
                    lighting_host::share_flow::virtual_prepare_abort_message(
                        &format!("{} [{reason}]", lighting_host::ui_text::human_last_error(&reason))
                    )
                );
            }
        }
    } else if let Err(err) = displays::apply_project_mode(req.share_mode) {
        tracing::warn!("DisplaySwitch failed ({err:#}); continuing with current layout");
    }

    set_status(&status, "启动", "正在枚举显示器");
    let displays = displays::list_displays()?;
    req.display_index = displays::pick_display_index(&displays, req.share_mode)
        .context("所选投屏模式没有可用显示器，请刷新列表")?;
    let display = displays
        .get(req.display_index)
        .cloned()
        .context("所选显示器不存在，请刷新列表")?;

    let ffmpeg = encoder::find_ffmpeg()?;
    let bind = if req.bind.is_empty() {
        format!("0.0.0.0:{}", protocol::PORT)
    } else {
        req.bind.clone()
    };

    set_status(&status, "监听", format!("绑定 {bind}"));
    let listener = TcpListener::bind(&bind).await.context("绑定端口")?;

    // DXGI cannot capture the lock screen. Hold the display/system idle
    // timeout for the whole share (including wait-for-tablet).
    let _keep_awake = displays::KeepAwakeGuard::acquire();
    let _lid = if req.share_mode.blanks_pc_monitor() {
        Some(displays::LidCloseGuard::apply())
    } else {
        None
    };
    let tablet_only = Arc::new(AtomicBool::new(false));
    let _restore_pc = TabletOnlyRestoreGuard(tablet_only.clone(), preserve.clone());

    let adb_path = adb::find_adb().ok();
    let mut reverse_serial: Option<String> = None;
    let mut wait_detail = "请在平板上打开 Lighting 并连接".to_string();
    if let Some(adb_bin) = adb_path.as_ref() {
        let devices = adb::list_devices(adb_bin).await.unwrap_or_default();
        let serial = req.device_serial.clone().or_else(|| {
            devices
                .into_iter()
                .find(|d| d.state == "device")
                .map(|d| d.serial)
        });
        if let Some(serial) = serial {
            set_status(
                &status,
                "等待设备",
                format!("正在执行 adb reverse（{serial}）"),
            );
            if let Err(err) = adb::reverse_port(adb_bin, &serial, protocol::PORT).await {
                set_transport(
                    &status,
                    "USB · adb reverse 失败，可改用 Wi-Fi（平板填电脑 IP）",
                );
                wait_detail = format!("adb reverse 失败，仍可走局域网：{err:#}");
            } else {
                reverse_serial = Some(serial.clone());
                set_transport(&status, format!("USB · adb reverse 已就绪（{serial}）"));
            }
        } else {
            set_transport(&status, "USB · 未检测到已授权设备，可走 Wi-Fi（填电脑 IP）");
            wait_detail = "未检测到已授权设备，平板可填电脑 IP".into();
        }
    } else {
        set_transport(&status, "未找到 adb · 仅局域网可用（平板填电脑 IP）");
        wait_detail = "未找到 adb，平板可填电脑 IP 用 Wi-Fi 测试".into();
    }

    set_status(&status, "等待设备", wait_detail);

    let (stop_tx, stop_rx) = watch::channel(false);
    let stop2 = stop.clone();
    tokio::spawn(async move {
        while !stop2.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let _ = stop_tx.send(true);
    });

    // Accept never stops while the share is running. Incoming reconnects are
    // parked here during the previous client's teardown (ffmpeg wait / adb).
    let (conn_tx, mut conn_rx) = mpsc::channel(1);
    let mut accept_stop = stop_rx.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = accept_stop.changed() => break,
                accepted = listener.accept() => {
                    match accepted {
                        Ok(pair) => {
                            tokio::select! {
                                _ = accept_stop.changed() => break,
                                sent = conn_tx.send(pair) => {
                                    if sent.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!("accept failed: {err:#}");
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                }
            }
        }
    });

    let mut stop_rx = stop_rx;
    loop {
        if !session_policy::continue_accept_loop(stop.load(Ordering::Relaxed)) {
            cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref()).await;
            return Ok(());
        }

        let stream = tokio::select! {
            _ = stop_rx.changed() => {
                cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref()).await;
                return Ok(());
            }
            incoming = conn_rx.recv() => {
                let Some((s, addr)) = incoming else {
                    cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref()).await;
                    anyhow::bail!("listen loop ended");
                };
                if let Ok(mut st) = status.lock() {
                    clear_peer_metrics(&mut st);
                    st.client_addr = addr.to_string();
                }
                set_status(&status, "已连接", format!("{addr}"));
                s
            }
        };

        match handle_client(
            stream,
            display.clone(),
            ffmpeg.clone(),
            req.clone(),
            status.clone(),
            stop.clone(),
            controls.clone(),
            tablet_only.clone(),
            preserve.clone(),
        )
        .await
        {
            Ok(()) => {
                tracing::info!("client session ended");
            }
            Err(err) => {
                tracing::warn!("client session ended: {err:#}");
            }
        }

        // Tablet drop / capture interrupt: put the laptop back as primary with
        // its original Hz so the user is not stuck on a blank panel + CAD.
        if let Some(snap) = preserve.clone() {
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(err) = displays::reassert_primary(&snap) {
                    tracing::warn!("reassert primary after tablet drop: {err:#}");
                }
            })
            .await;
        }

        if !session_policy::continue_accept_loop(stop.load(Ordering::Relaxed)) {
            cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref()).await;
            return Ok(());
        }

        // Refresh reverse in the background so USB 127.0.0.1 is restored
        // without stalling the already-running accept task.
        if let (Some(adb_bin), Some(serial)) = (adb_path.clone(), reverse_serial.clone()) {
            tokio::spawn(async move {
                if let Err(err) = adb::reverse_port(&adb_bin, &serial, protocol::PORT).await {
                    tracing::warn!("re-apply adb reverse failed: {err:#}");
                }
            });
        }
        if let Ok(mut st) = status.lock() {
            clear_peer_metrics(&mut st);
        }
        set_status(&status, "等待设备", "上一台已断开，等待重新连接");
    }
}

async fn cleanup_reverse(adb: Option<&std::path::PathBuf>, serial: Option<&str>) {
    if let (Some(adb), Some(serial)) = (adb, serial) {
        let _ = adb::remove_reverse(adb, serial, protocol::PORT).await;
    }
}

struct TabletOnlyRestoreGuard(Arc<AtomicBool>, Option<displays::PrimarySnapshot>);

impl Drop for TabletOnlyRestoreGuard {
    fn drop(&mut self) {
        if self.0.swap(false, Ordering::SeqCst) {
            if let Some(snap) = self.1.take() {
                if let Err(err) = displays::restore_desktop(&snap) {
                    tracing::warn!("restore PC monitor after tablet-only failed: {err:#}");
                } else {
                    tracing::info!("restored PC monitor after tablet-only session");
                }
            } else if let Err(err) = displays::restore_pc_monitor() {
                tracing::warn!("restore PC monitor after tablet-only failed: {err:#}");
            }
        }
    }
}

async fn handle_client(
    stream: TcpStream,
    mut display: DisplayInfo,
    ffmpeg: std::path::PathBuf,
    req: SessionRequest,
    status: Arc<Mutex<SessionStatus>>,
    stop: Arc<AtomicBool>,
    controls: Arc<Controls>,
    tablet_only: Arc<AtomicBool>,
    preserve: Option<displays::PrimarySnapshot>,
) -> Result<()> {
    stream.set_nodelay(true)?;
    let (mut reader, mut writer) = stream.into_split();

    let hello_msg = protocol::read_message(&mut reader)
        .await
        .context("读 Hello")?;
    if hello_msg.ty != protocol::MSG_HELLO {
        anyhow::bail!("首包不是 Hello");
    }
    let hello: Hello = serde_json::from_slice(&hello_msg.payload).context("解析 Hello")?;
    tracing::info!("hello: {:?}", hello);
    if let Ok(mut s) = status.lock() {
        s.client_name = hello.device.trim().to_string();
    }

    let mut mode_guard = displays::ModeRestoreGuard(None);
    if req.share_mode.uses_virtual_display() && hello.screen_width > 0 && hello.screen_height > 0 {
        // Independent second screen: put the *virtual* monitor on tablet pixels so
        // capture is 1:1 (no scaling anywhere) and the PC monitor is untouched.
        set_status(
            &status,
            "独立第二屏",
            format!(
                "正在把虚拟屏设为平板分辨率 {}×{}",
                hello.screen_width, hello.screen_height
            ),
        );
        let (tw, th) = (hello.screen_width, hello.screen_height);
        let want_fps = hello.max_fps.max(req.fps).min(120);
        let preserve_for_mode = preserve.clone();
        match tokio::task::spawn_blocking(move || {
            displays::configure_virtual_for_tablet(tw, th, want_fps, preserve_for_mode.as_ref())
        })
        .await
        {
            Ok(Ok(updated)) => {
                tracing::info!(
                    "virtual display now {}×{} (capture {:?})",
                    updated.width,
                    updated.height,
                    updated.dxgi
                );
                display = updated;
                set_status(
                    &status,
                    "独立第二屏",
                    format!("虚拟屏 {}×{} · 1:1 抓取", display.width, display.height),
                );
            }
            Ok(Err(err)) => {
                tracing::warn!("configure virtual for tablet failed: {err:#}");
                set_status(
                    &status,
                    "独立第二屏",
                    format!("虚拟屏未能设为平板分辨率，将缩放推流。{err}"),
                );
            }
            Err(err) => {
                tracing::warn!("configure virtual join failed: {err:#}");
            }
        }
    } else if req.match_device && hello.screen_width > 0 && hello.screen_height > 0 {
        // Mirror + 跟随平板: temporarily switch the captured PC monitor toward the
        // tablet panel so Windows「显示设置」matches. Refresh rate is protected —
        // dropping a high-Hz panel to 60 Hz reads as stutter.
        let (tw, th) = lighting_host::session_policy::orient_box(
            display.width,
            display.height,
            hello.screen_width,
            hello.screen_height,
        );
        let prefer_fps = req
            .fps
            .max(30)
            .min(hello.max_fps.max(60))
            .min(120);
        let device = display.name.clone();
        let current = displays::DisplayMode {
            width: display.width,
            height: display.height,
            fps: prefer_fps,
        };
        set_status(
            &status,
            "适配平板",
            format!("正在把电脑分辨率切到平板面板 {tw}×{th}…"),
        );
        let switched = tokio::task::spawn_blocking(move || {
            displays::apply_follow_tablet_mode(&device, current, tw, th, prefer_fps)
        })
        .await;
        match switched {
            Ok(Ok((applied, restore))) => {
                let changed =
                    restore.mode.width != applied.width || restore.mode.height != applied.height;
                if changed {
                    mode_guard.0 = Some(restore);
                }
                let device_name = display.name.clone();
                if let Ok(list) = displays::list_displays() {
                    if let Some(updated) = list
                        .iter()
                        .find(|d| d.name == device_name)
                        .cloned()
                    {
                        display = updated;
                    } else {
                        display.width = applied.width;
                        display.height = applied.height;
                    }
                } else {
                    display.width = applied.width;
                    display.height = applied.height;
                }
                set_status(
                    &status,
                    "适配平板",
                    format!(
                        "电脑分辨率已切换为 {}×{}（跟随平板 {}×{}）",
                        display.width, display.height, hello.screen_width, hello.screen_height
                    ),
                );
            }
            Ok(Err(err)) => {
                tracing::warn!("follow-tablet mode switch failed: {err:#}");
                set_status(
                    &status,
                    "适配平板",
                    format!(
                        "电脑屏无法切到平板分辨率，已改为缩放推流（显示设置仍可能是电脑分辨率）。{err}"
                    ),
                );
            }
            Err(err) => {
                tracing::warn!("follow-tablet mode switch join failed: {err:#}");
            }
        }
    } else if hello.screen_width > 0 && hello.screen_height > 0 {
        set_status(
            &status,
            "适配平板",
            format!(
                "按平板分辨率编码 {}×{}",
                hello.screen_width, hello.screen_height
            ),
        );
    }

    if req.share_mode.blanks_pc_monitor() {
        set_status(&status, "仅平板", "正在关闭电脑屏（Win+P 仅第二屏幕）…");
        match tokio::task::spawn_blocking(displays::apply_tablet_only_output).await {
            Ok(Ok(())) => {
                tablet_only.store(true, Ordering::SeqCst);
                let list = displays::list_displays()?;
                display = list.into_iter()
                    .find(|d| d.name == display.name)
                    .context("切换仅平板后原扩展屏已断开")?;
                set_status(
                    &status,
                    "仅平板",
                    format!(
                        "电脑屏已关 · 捕获 {}×{}。合盖可用；请勿锁屏。",
                        display.width, display.height
                    ),
                );
            }
            Ok(Err(err)) => {
                tracing::warn!("tablet-only DisplaySwitch /external failed: {err:#}");
                set_status(
                    &status,
                    "仅平板",
                    format!("未能关闭电脑屏，将继续双屏推流。{err}"),
                );
            }
            Err(err) => {
                tracing::warn!("tablet-only join failed: {err:#}");
            }
        }
    }

    let codec = pick_codec(&hello, req.prefer_hevc);
    let (dec_w, dec_h, dec_fps, hw) = codec_limit(&hello, &codec);
    // Always clamp to the tablet panel when Hello reports it — a 2K desktop
    // must not stream 2K to a 1080p/1200p pad just because ResCap is「最高 2K」.
    let scale = if req.match_device || req.share_mode.uses_virtual_display() {
        req.scale
    } else {
        1.0
    };
    let (mut width, mut height) = lighting_host::session_policy::compute_encode_size(
        display.width,
        display.height,
        hello.screen_width,
        hello.screen_height,
        req.max_width,
        req.max_height,
        scale,
        dec_w,
        dec_h,
    );
    let align = hello.alignment.max(2);
    width = (width / align * align).max(align);
    height = (height / align * align).max(align);

    let fps = adapted_fps(req.fps, hello.max_fps, dec_fps, hw);
    let auto_br = auto_bitrate(width, height, fps);
    // Honor the user's bitrate for hardware decode — auto_br used to silently
    // cap 1080p@60 to ~15 Mbps even when the slider said 25 Mbps.
    let bitrate_kbps = if hw {
        req.bitrate_kbps.clamp(4_000, 80_000).max(auto_br.min(req.bitrate_kbps))
    } else {
        req.bitrate_kbps.min(12_000).clamp(4_000, 12_000)
    };
    let audio_enabled = req.send_audio;
    let cfg = StreamConfig {
        width,
        height,
        fps,
        codec: codec.clone(),
        bitrate_kbps,
        audio_enabled,
        audio_sample_rate: 48000,
        audio_channels: 2,
        host_name: protocol::host_name(),
    };
    let payload = serde_json::to_vec(&cfg)?;
    protocol::write_message(&mut writer, protocol::MSG_CONFIG, 0, &payload).await?;
    if let Ok(mut s) = status.lock() {
        s.codec = codec.clone();
        s.width = width;
        s.height = height;
        s.fps = fps;
    }

    set_status(
        &status,
        "编码",
        format!(
            "{codec} {width}×{height}@{fps} {br} kbps{audio} [{soc}{gsi}{hw}]",
            br = cfg.bitrate_kbps,
            audio = if audio_enabled { " + 音频" } else { "" },
            soc = if hello.soc.is_empty() {
                "soc?"
            } else {
                &hello.soc
            },
            gsi = if hello.gsi { " GSI" } else { "" },
            hw = if hw {
                format!(" 硬解{dec_w}×{dec_h}@{dec_fps}")
            } else {
                " 软解".into()
            }
        ),
    );
    set_bitrate(&status, cfg.bitrate_kbps);

    let settings = EncodeSettings {
        width,
        height,
        fps,
        bitrate_kbps: cfg.bitrate_kbps,
        codec: codec.clone(),
        encoder: encoder::pick_encoder(&codec).into(),
        profile: if codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265") {
            "main".into()
        } else if hw {
            "main".into()
        } else {
            "baseline".into()
        },
    };

    let hevc = codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265");
    let (mut session, bootstrap) =
        start_live_encoder(&ffmpeg, &display, &settings, hevc)?;
    let t0 = std::time::Instant::now();
    let audio_stop = Arc::new(AtomicBool::new(false));
    let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel::<crate::audio::AudioPacket>(4);
    if audio_enabled {
        match crate::audio::start_loopback(audio_tx, audio_stop.clone(), t0) {
            Ok(()) => tracing::info!("audio loopback started"),
            Err(err) => {
                tracing::warn!("audio loopback unavailable: {err:#}");
            }
        }
    }

    let display_for_input = display.clone();
    let (touch_tx, touch_rx) = std::sync::mpsc::channel::<protocol::TouchEvent>();
    let input_display = display_for_input.clone();
    std::thread::Builder::new()
        .name("lighting-input".into())
        .spawn(move || {
            while let Ok(ev) = touch_rx.recv() {
                input::inject_touch(&input_display, ev);
            }
        })
        .ok();
    let stop_read = stop.clone();
    let ping_sent: Arc<Mutex<Option<std::time::Instant>>> = Arc::new(Mutex::new(None));
    let ping_reply = ping_sent.clone();
    let status_read = status.clone();
    let controls_read = controls.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            if stop_read.load(Ordering::Relaxed) {
                break;
            }
            match protocol::read_message(&mut reader).await {
                Ok(msg) if msg.ty == protocol::MSG_TOUCH => {
                    if !controls_read.touch.load(Ordering::Relaxed) {
                        tracing::warn!("touch ignored: relay disabled");
                        continue;
                    }
                    match protocol::TouchEvent::parse(&msg.payload) {
                        Ok(ev) => {
                            tracing::info!("host got touch action={} x={} y={}", ev.action, ev.x, ev.y);
                            if touch_tx.send(ev).is_err() {
                                tracing::warn!("touch queue closed");
                            }
                        }
                        Err(err) => tracing::warn!("bad touch payload: {err:#}"),
                    }
                }
                Ok(msg) if msg.ty == protocol::MSG_HEARTBEAT => {
                    let rtt = ping_reply
                        .lock()
                        .ok()
                        .and_then(|mut slot| slot.take())
                        .map(|sent| sent.elapsed().as_millis().min(9_999) as u32);
                    if let (Some(rtt), Ok(mut s)) = (rtt, status_read.lock()) {
                        s.latency_ms = session_policy::smooth_latency_ms(s.latency_ms, rtt);
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    match write_bootstrap(&mut writer, t0, &bootstrap).await {
        Ok(sent) => add_wire_bytes(&status, sent),
        Err(err) => anyhow::bail!("发送首帧失败: {err:#}"),
    }

    let mut last_ping = std::time::Instant::now();
    while !stop.load(Ordering::Relaxed) {
        while let Ok(pkt) = audio_rx.try_recv() {
            let payload = protocol::with_pts(pkt.pts_us, &pkt.pcm);
            if let Err(err) =
                protocol::write_message(&mut writer, protocol::MSG_AUDIO, 0, &payload).await
            {
                tracing::warn!("send audio failed: {err:#}");
                break;
            }
            add_wire_bytes(&status, payload.len() + HEADER_BYTES);
        }
        if last_ping.elapsed() >= Duration::from_millis(1_000) {
            last_ping = std::time::Instant::now();
            if protocol::write_message(&mut writer, protocol::MSG_HEARTBEAT, 0, &[])
                .await
                .is_ok()
            {
                if let Ok(mut slot) = ping_sent.lock() {
                    slot.get_or_insert_with(std::time::Instant::now);
                }
                // Keeps the footer clock ticking even if the encoder stalls.
                if let Ok(mut s) = status.lock() {
                    s.connected_secs = t0.elapsed().as_secs();
                }
            }
        }
        match session.rx.recv_timeout(Duration::from_millis(1)) {
            Ok(pkt) => {
                let mut sent = match write_video_packet(&mut writer, t0, &pkt).await {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        tracing::warn!("send video failed: {err:#}");
                        break;
                    }
                };
                while let Ok(ap) = audio_rx.try_recv() {
                    let audio_payload = protocol::with_pts(ap.pts_us, &ap.pcm);
                    if protocol::write_message(&mut writer, protocol::MSG_AUDIO, 0, &audio_payload)
                        .await
                        .is_err()
                    {
                        break;
                    }
                    sent += audio_payload.len() + HEADER_BYTES;
                }
                if let Ok(mut s) = status.lock() {
                    s.frames += 1;
                    s.bytes_sent += sent as u64;
                    s.connected_secs = t0.elapsed().as_secs();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!("encoder pipe closed, trying gdigrab once");
                set_status(&status, "回退", "改用 gdigrab 抓屏");
                session.stop();
                match restart_encoder_with_bootstrap(&ffmpeg, &display, &settings, hevc) {
                    Ok((new_session, bootstrap)) => {
                        session = new_session;
                        match write_bootstrap(&mut writer, t0, &bootstrap).await {
                            Ok(sent) => add_wire_bytes(&status, sent),
                            Err(err) => {
                                tracing::warn!(
                                    "send bootstrap after encoder restart failed: {err:#}"
                                );
                                break;
                            }
                        }
                        set_status(&status, "编码", "gdigrab 已重发 codec-config + IDR");
                    }
                    Err(err) => {
                        tracing::warn!("gdigrab 回退失败: {err:#}");
                        set_status(&status, "错误", "gdigrab 回退也失败，请查看日志");
                        break;
                    }
                }
            }
        }
    }

    audio_stop.store(true, Ordering::Relaxed);
    reader_task.abort();
    let _ = writer.shutdown().await;
    session.stop_in_background();
    // Restore PC resolution after capture stops so DXGI isn't mid-grab.
    drop(mode_guard);
    Ok(())
}

fn video_flags(pkt: &EncodedPacket) -> u8 {
    let mut flags = 0u8;
    if pkt.keyframe {
        flags |= FLAG_KEYFRAME;
    }
    if pkt.codec_config {
        flags |= FLAG_CODEC_CONFIG;
    }
    flags
}

/// Returns the bytes put on the wire so the 已传输 counter stays honest.
async fn write_video_packet<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    t0: std::time::Instant,
    pkt: &EncodedPacket,
) -> Result<usize> {
    let payload = protocol::with_pts(t0.elapsed().as_micros() as u64, &pkt.data);
    protocol::write_message(writer, protocol::MSG_VIDEO, video_flags(pkt), &payload).await?;
    Ok(payload.len() + HEADER_BYTES)
}

async fn write_bootstrap<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    t0: std::time::Instant,
    packets: &[EncodedPacket],
) -> Result<usize> {
    let mut sent = 0;
    for pkt in packets {
        sent += write_video_packet(writer, t0, pkt).await?;
    }
    Ok(sent)
}

fn start_live_encoder(
    ffmpeg: &std::path::PathBuf,
    display: &DisplayInfo,
    settings: &EncodeSettings,
    hevc: bool,
) -> Result<(encoder::EncoderSession, Vec<EncodedPacket>)> {
    if lighting_host::session_policy::prefer_gdigrab_capture(
        display.is_virtual,
        display.dxgi.is_some(),
    ) {
        match restart_encoder_with_bootstrap(ffmpeg, display, settings, hevc) {
            Ok(ok) => return Ok(ok),
            Err(err) => tracing::warn!("gdigrab-first for virtual display failed: {err:#}"),
        }
    }
    let mut last_err: Option<anyhow::Error> = None;
    for enc in encoder::encoder_fallback_chain(&settings.codec) {
        let graphs = lighting_host::capture_graph::dda_capture_graphs(
            display.dxgi,
            settings.fps,
            display.width,
            display.height,
            settings.width,
            settings.height,
            enc,
        );
        for graph in graphs {
            let session = match encoder::start_encoder(ffmpeg, display, settings, enc, &graph) {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!("encoder {enc} spawn failed: {err:#}");
                    last_err = Some(err);
                    continue;
                }
            };
            match annexb::recv_bootstrap(&session.rx, Duration::from_secs(3), hevc) {
                Ok(bootstrap) => {
                    tracing::info!("using encoder {enc} graph={graph}");
                    return Ok((session, bootstrap));
                }
                Err(err) => {
                    tracing::warn!("{enc} graph died before codec-config + IDR ({graph}): {err:#}");
                    last_err = Some(err);
                }
            }
        }
    }
    tracing::warn!("desktop duplication encoders failed, trying gdigrab: {:?}", last_err);
    restart_encoder_with_bootstrap(ffmpeg, display, settings, hevc)
}

fn restart_encoder_with_bootstrap(
    ffmpeg: &std::path::PathBuf,
    display: &DisplayInfo,
    settings: &EncodeSettings,
    hevc: bool,
) -> Result<(encoder::EncoderSession, Vec<EncodedPacket>)> {
    let mut last_err: Option<anyhow::Error> = None;
    for enc in encoder::encoder_fallback_chain(&settings.codec) {
        let session = match encoder::start_encoder_gdigrab(ffmpeg, display, settings, enc) {
            Ok(s) => s,
            Err(err) => {
                tracing::warn!("gdigrab encoder {enc} failed: {err:#}");
                last_err = Some(err);
                continue;
            }
        };
        match annexb::recv_bootstrap(&session.rx, Duration::from_secs(3), hevc) {
            Ok(bootstrap) => {
                tracing::info!("gdigrab bootstrap ok with {enc}");
                return Ok((session, bootstrap));
            }
            Err(err) => {
                tracing::warn!("{enc} closed before codec-config + IDR: {err:#}");
                last_err = Some(err);
            }
        }
    }
    anyhow::bail!(
        "encoder pipe closed before codec-config + IDR: {:?}",
        last_err
    )
}

fn codec_limit(hello: &Hello, codec: &str) -> (u32, u32, u32, bool) {
    let picked = if codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265") {
        hello.hevc_limit.as_ref()
    } else {
        hello.avc_limit.as_ref()
    };
    if let Some(l) = picked {
        if l.width > 0 && l.height > 0 {
            return (l.width, l.height, l.fps.max(24), l.hw);
        }
    }
    (
        hello.decoder_max_width,
        hello.decoder_max_height,
        hello.decoder_max_fps,
        hello.hw_decode,
    )
}

fn pick_codec(hello: &Hello, prefer_hevc: bool) -> String {
    let has = |n: &str| hello.codecs.iter().any(|c| c.eq_ignore_ascii_case(n));
    let avc_ok = has("avc") || has("h264") || hello.avc_limit.is_some();
    let hevc_ok = has("hevc") || has("h265") || hello.hevc_limit.is_some();
    let avc_score = if avc_ok {
        codec_score(
            hello.avc_limit.as_ref(),
            hello.decoder_max_width,
            hello.decoder_max_height,
            hello.decoder_max_fps,
        )
    } else {
        0
    };
    let hevc_score = if hevc_ok {
        codec_score(
            hello.hevc_limit.as_ref(),
            hello.decoder_max_width,
            hello.decoder_max_height,
            hello.decoder_max_fps,
        )
    } else {
        0
    };
    let screen_area = (hello.screen_width.max(1) as u64).saturating_mul(hello.screen_height.max(1) as u64);
    let avc_area = hello
        .avc_limit
        .as_ref()
        .map(|l| (l.width.max(1) as u64).saturating_mul(l.height.max(1) as u64))
        .unwrap_or_else(|| {
            (hello.decoder_max_width.max(1) as u64).saturating_mul(hello.decoder_max_height.max(1) as u64)
        });
    let hevc_area = hello
        .hevc_limit
        .as_ref()
        .map(|l| (l.width.max(1) as u64).saturating_mul(l.height.max(1) as u64))
        .unwrap_or(0);

    // Treble/GSI HEVC is often broken or much slower; stay on AVC unless HEVC is clearly larger.
    if hello.gsi && avc_ok && hevc_score < avc_score.saturating_mul(2) {
        return "avc".into();
    }
    // Tablet panel larger than AVC hard-decode ceiling → pick HEVC when it unlocks more pixels.
    if hevc_ok && avc_ok && screen_area > avc_area && hevc_area > avc_area {
        return "hevc".into();
    }
    if prefer_hevc && hevc_ok && hevc_score >= avc_score {
        return "hevc".into();
    }
    if hevc_ok && !avc_ok {
        return "hevc".into();
    }
    if avc_ok {
        "avc".into()
    } else if hevc_ok {
        "hevc".into()
    } else {
        "avc".into()
    }
}

fn codec_score(limit: Option<&crate::protocol::CodecLimit>, fw: u32, fh: u32, ffps: u32) -> u64 {
    if let Some(l) = limit {
        if l.width > 0 && l.height > 0 {
            return (l.width as u64)
                .saturating_mul(l.height as u64)
                .saturating_mul(l.fps.max(24) as u64);
        }
    }
    (fw as u64)
        .saturating_mul(fh as u64)
        .saturating_mul(ffps.max(24) as u64)
}

fn adapted_fps(req_fps: u32, hello_max: u32, dec_fps: u32, hw: bool) -> u32 {
    let mut fps = req_fps.min(hello_max.max(24)).min(120).max(24);
    if dec_fps >= 24 {
        fps = fps.min(dec_fps);
    }
    if !hw {
        fps = fps.min(45);
    }
    fps
}

fn fit_to_device(
    src_w: u32,
    src_h: u32,
    dev_w: u32,
    dev_h: u32,
    scale: f32,
    dec_w: u32,
    dec_h: u32,
) -> (u32, u32) {
    lighting_host::session_policy::compute_encode_size(
        src_w,
        src_h,
        dev_w,
        dev_h,
        u32::MAX / 4,
        u32::MAX / 4,
        scale,
        dec_w,
        dec_h,
    )
}

fn auto_bitrate(width: u32, height: u32, fps: u32) -> u32 {
    let mp = (width as f64 * height as f64) / 1_000_000.0;
    let kbps = (mp * fps as f64 * 120.0) as u32;
    kbps.clamp(6_000, 40_000)
}

fn fit_resolution(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    lighting_host::session_policy::fit_resolution(src_w, src_h, max_w, max_h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{CodecLimit, Hello};

    fn qcom_gsi() -> Hello {
        Hello {
            protocol: 1,
            device: "Lineage TrebleDroid".into(),
            screen_width: 2000,
            screen_height: 1200,
            max_fps: 60,
            codecs: vec!["avc".into(), "hevc".into()],
            want_audio: true,
            decoder_max_width: 1920,
            decoder_max_height: 1088,
            decoder_max_fps: 60,
            hw_decode: true,
            alignment: 2,
            soc: "qcom".into(),
            gsi: true,
            brand: "lineage".into(),
            avc_limit: Some(CodecLimit {
                width: 1920,
                height: 1088,
                fps: 60,
                hw: true,
                name: "OMX.qcom.video.decoder.avc".into(),
            }),
            hevc_limit: Some(CodecLimit {
                width: 1280,
                height: 720,
                fps: 30,
                hw: true,
                name: "OMX.qcom.video.decoder.hevc".into(),
            }),
        }
    }

    #[test]
    fn hevc_uses_hevc_limit() {
        let h = qcom_gsi();
        assert_eq!(codec_limit(&h, "hevc"), (1280, 720, 30, true));
        assert_eq!(codec_limit(&h, "avc"), (1920, 1088, 60, true));
    }

    #[test]
    fn qcom_1080p_caps_1440p_desktop() {
        let (w, h) = fit_to_device(2560, 1440, 2000, 1200, 1.0, 1920, 1088);
        assert!(w <= 1920 && h <= 1088);
        assert_eq!(w % 2, 0);
        assert_eq!((w, h), (1920, 1080));
    }

    #[test]
    fn gsi_stays_on_avc_even_if_hevc_preferred() {
        let h = qcom_gsi();
        assert_eq!(pick_codec(&h, true), "avc");
        assert_eq!(pick_codec(&h, false), "avc");
    }

    #[test]
    fn weak_hevc_tablet_stays_on_avc() {
        // Retail 2020-class pad: AVC 1080p60 hardware, HEVC only 720p30.
        let mut h = qcom_gsi();
        h.gsi = false;
        assert_eq!(pick_codec(&h, true), "avc");
        assert_eq!(pick_codec(&h, false), "avc");
    }

    #[test]
    fn hevc_wins_when_clearly_higher_res() {
        let mut h = qcom_gsi();
        h.gsi = false;
        h.hevc_limit = Some(CodecLimit {
            width: 3840,
            height: 2160,
            fps: 60,
            hw: true,
            name: "c2.mtk.hevc.decoder".into(),
        });
        assert_eq!(pick_codec(&h, true), "hevc");
        // Screen 2000×1200 exceeds AVC 1920×1088 → auto HEVC even without prefer flag.
        assert_eq!(pick_codec(&h, false), "hevc");
    }

    #[test]
    fn hevc_not_forced_when_screen_fits_avc() {
        let mut h = qcom_gsi();
        h.gsi = false;
        h.screen_width = 1920;
        h.screen_height = 1080;
        h.hevc_limit = Some(CodecLimit {
            width: 3840,
            height: 2160,
            fps: 60,
            hw: true,
            name: "c2.qti.hevc.decoder".into(),
        });
        assert_eq!(pick_codec(&h, false), "avc");
        assert_eq!(pick_codec(&h, true), "hevc");
    }

    #[test]
    fn user_bitrate_not_silently_capped_on_hw() {
        // Historical bug: auto_bitrate(1920×1080@60) ≈ 15 Mbps crushed a 25 Mbps slider.
        let auto = auto_bitrate(1920, 1080, 60);
        assert!(auto < 25_000);
        let user = 25_000u32;
        let hw_br = user.clamp(4_000, 80_000).max(auto.min(user));
        assert_eq!(hw_br, 25_000);
    }

    #[test]
    fn software_decode_caps_fps() {
        assert_eq!(adapted_fps(60, 60, 60, false), 45);
        assert_eq!(adapted_fps(120, 60, 60, true), 60);
        assert_eq!(adapted_fps(60, 60, 30, true), 30);
    }

    #[test]
    fn live_transport_only_while_running() {
        assert_eq!(
            live_transport(true, "USB · adb reverse 已就绪"),
            Some("USB · adb reverse 已就绪")
        );
        assert_eq!(live_transport(false, "USB · adb reverse 已就绪"), None);
        assert_eq!(live_transport(true, ""), None);
        assert_eq!(live_transport(false, ""), None);
    }

    #[test]
    fn avc_level_matches_common_phones() {
        assert_eq!(crate::encoder::avc_level(1920, 1080, 60), "4.2");
        assert_eq!(crate::encoder::avc_level(1280, 720, 60), "3.2");
        assert_eq!(crate::encoder::avc_level(2560, 1440, 60), "5.0");
    }

    #[test]
    fn video_flags_mark_config_and_idr() {
        let cfg = EncodedPacket {
            data: vec![0, 0, 0, 1, 0x67],
            keyframe: false,
            codec_config: true,
        };
        let idr = EncodedPacket {
            data: vec![0, 0, 0, 1, 0x65],
            keyframe: true,
            codec_config: false,
        };
        assert_eq!(video_flags(&cfg), FLAG_CODEC_CONFIG);
        assert_eq!(video_flags(&idr), FLAG_KEYFRAME);
        assert!(session_policy::continue_accept_loop(false));
        assert!(!session_policy::continue_accept_loop(true));
    }
}
