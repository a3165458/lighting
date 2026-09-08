use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, Notify};

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

/// What `handle_client` did with this Hello. Virtual mode change on Honor
/// kills USB, so that path must not restore the laptop desktop (CCD) or
/// start ffmpeg on the dead socket.
enum ClientOutcome {
    Finished,
    NeedHelloAfterVirtualMode,
}

struct ClassifiedStream {
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    hello: Hello,
    addr: std::net::SocketAddr,
}

fn apply_socket_buffers(stream: &TcpStream, send: usize, recv: usize) {
    use std::os::windows::io::AsRawSocket;
    use windows::Win32::Networking::WinSock::{
        setsockopt, SOCKET, SOL_SOCKET, SO_RCVBUF, SO_SNDBUF,
    };
    unsafe {
        let s = SOCKET(stream.as_raw_socket() as usize);
        let snd = (send as i32).to_le_bytes();
        let rcv = (recv as i32).to_le_bytes();
        let _ = setsockopt(s, SOL_SOCKET, SO_SNDBUF, Some(&snd));
        let _ = setsockopt(s, SOL_SOCKET, SO_RCVBUF, Some(&rcv));
    }
}

/// scrcpy / Moonlight: NODELAY after connect *and* after SO_SNDBUF (some
/// stacks drop the flag). SIO_TCP_SET_ACK_FREQUENCY=1 is the Windows
/// equivalent of the tablet's TCP_QUICKACK — default delayed ACK is 200 ms.
fn apply_tcp_low_delay(stream: &TcpStream) {
    use std::os::windows::io::AsRawSocket;
    use windows::Win32::Networking::WinSock::{
        setsockopt, WSAIoctl, IPPROTO_TCP, SIO_TCP_SET_ACK_FREQUENCY, SOCKET, TCP_NODELAY,
    };
    unsafe {
        let s = SOCKET(stream.as_raw_socket() as usize);
        let nodelay = 1i32.to_le_bytes();
        let _ = setsockopt(s, IPPROTO_TCP.0, TCP_NODELAY, Some(&nodelay));
        if !session_policy::tcp_ack_every_packet() {
            return;
        }
        let freq: u32 = 1;
        let mut returned = 0u32;
        let _ = WSAIoctl(
            s,
            SIO_TCP_SET_ACK_FREQUENCY,
            Some((&freq as *const u32).cast()),
            std::mem::size_of::<u32>() as u32,
            None,
            0,
            &mut returned,
            None,
            None,
        );
    }
}

async fn classify_incoming(
    mut stream: TcpStream,
    addr: std::net::SocketAddr,
) -> Result<ClassifiedStream> {
    stream.set_nodelay(true)?;
    let hello_msg =
        tokio::time::timeout(Duration::from_secs(3), protocol::read_message(&mut stream))
            .await
            .context("Hello 超时")?
            .context("读 Hello")?;
    if hello_msg.ty != protocol::MSG_HELLO {
        anyhow::bail!("首包不是 Hello");
    }
    let hello: Hello = serde_json::from_slice(&hello_msg.payload).context("解析 Hello")?;
    // Video AUs need ~1–2 frames of buffer; control stays tiny so cursor
    // cannot sit behind 80 ms of TCP bufferbloat.
    let (send, recv) = if session_policy::hello_is_control_plane(&hello.role) {
        let n = session_policy::tcp_control_buffer_bytes();
        (n, n)
    } else {
        (
            session_policy::tcp_send_buffer_bytes(),
            session_policy::tcp_recv_buffer_bytes(),
        )
    };
    apply_socket_buffers(&stream, send, recv);
    let _ = stream.set_nodelay(true);
    apply_tcp_low_delay(&stream);
    let (reader, writer) = stream.into_split();
    Ok(ClassifiedStream {
        reader,
        writer,
        hello,
        addr,
    })
}

async fn flush_cursor(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    slot: &Arc<Mutex<Option<Vec<u8>>>>,
) -> Result<()> {
    let payload = slot.lock().ok().and_then(|mut g| g.take());
    if let Some(payload) = payload {
        protocol::write_message(writer, protocol::MSG_CURSOR, 0, &payload).await?;
    }
    Ok(())
}

fn spawn_cursor_control(
    incoming: ClassifiedStream,
    slot: Arc<Mutex<Option<Vec<u8>>>>,
    notify: Arc<Notify>,
    stop: Arc<AtomicBool>,
    touch_tx: std::sync::mpsc::Sender<protocol::TouchEvent>,
    controls: Arc<Controls>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut writer = incoming.writer;
        let mut reader = incoming.reader;
        while !stop.load(Ordering::Relaxed) {
            tokio::select! {
                msg = protocol::read_message(&mut reader) => {
                    match msg {
                        Ok(m) if m.ty == protocol::MSG_TOUCH => {
                            if !controls.touch.load(Ordering::Relaxed) {
                                continue;
                            }
                            match protocol::TouchEvent::parse(&m.payload) {
                                Ok(ev) => {
                                    if touch_tx.send(ev).is_err() {
                                        break;
                                    }
                                }
                                Err(err) => tracing::warn!("bad control touch: {err:#}"),
                            }
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                _ = notify.notified() => {
                    // tokio Notify stores a permit if this fires before we wait.
                    // A 1 ms poll here added a tick of pointer delay for no reason.
                    if flush_cursor(&mut writer, &slot).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = writer.shutdown().await;
    })
}

fn take_audio_packets(
    rx: &std::sync::mpsc::Receiver<crate::audio::AudioPacket>,
    max: usize,
) -> Vec<crate::audio::AudioPacket> {
    let mut out = Vec::new();
    let keep = lighting_host::session_policy::audio_packets_to_send(usize::MAX, max);
    while out.len() < keep {
        match rx.try_recv() {
            Ok(pkt) => out.push(pkt),
            Err(_) => break,
        }
    }
    out
}

fn encode_audio_packet(ap: &crate::audio::AudioPacket) -> Vec<u8> {
    let audio_payload = protocol::with_pts(ap.pts_us, &ap.pcm);
    session_policy::lit1_encode(protocol::MSG_AUDIO, 0, &audio_payload)
}

fn append_pending_audio(
    out: &mut Vec<u8>,
    rx: &std::sync::mpsc::Receiver<crate::audio::AudioPacket>,
) {
    let max = session_policy::audio_packets_per_video_frame();
    if max == 0 {
        return;
    }
    for ap in take_audio_packets(rx, max) {
        out.extend_from_slice(&encode_audio_packet(&ap));
    }
}

fn bind_control(
    ctrl: ClassifiedStream,
    slot: Arc<Mutex<Option<Vec<u8>>>>,
    notify: Arc<Notify>,
    stop: Arc<AtomicBool>,
    touch_tx: std::sync::mpsc::Sender<protocol::TouchEvent>,
    controls: Arc<Controls>,
) -> tokio::task::JoinHandle<()> {
    tracing::info!("cursor+HID on dedicated control socket (GlideX-style)");
    spawn_cursor_control(ctrl, slot, notify, stop, touch_tx, controls)
}

async fn attach_control_plane(
    ctrl_rx: &mut mpsc::Receiver<ClassifiedStream>,
    slot: Arc<Mutex<Option<Vec<u8>>>>,
    notify: Arc<Notify>,
    stop: Arc<AtomicBool>,
    touch_tx: std::sync::mpsc::Sender<protocol::TouchEvent>,
    controls: Arc<Controls>,
    wait: bool,
) -> Option<tokio::task::JoinHandle<()>> {
    if let Ok(ctrl) = ctrl_rx.try_recv() {
        return Some(bind_control(ctrl, slot, notify, stop, touch_tx, controls));
    }
    if !wait {
        return None;
    }
    match tokio::time::timeout(
        Duration::from_millis(session_policy::control_attach_wait_ms()),
        ctrl_rx.recv(),
    )
    .await
    {
        Ok(Some(ctrl)) => Some(bind_control(ctrl, slot, notify, stop, touch_tx, controls)),
        _ => None,
    }
}

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

    // USB listen + reverse MUST beat IddCx/UAC. The tablet reconnects to
    // 127.0.0.1 for only ~12s on 0.1.48; a 30s driver wait looks like
    // 「重连中 / 等待 USB」 even though adb devices is fine.
    let ffmpeg = encoder::find_ffmpeg()?;
    let bind = if req.bind.is_empty() {
        format!("0.0.0.0:{}", protocol::PORT)
    } else {
        req.bind.clone()
    };
    let listen_port = session_policy::listen_port_from_bind(&bind);

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
    let mut reverse_serial: Option<String> = req.device_serial.clone();

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
    // Hello.role == "control" is a GlideX-style pointer plane on the same port;
    // it must not enter the video session channel or it HOL-blocks the pointer.
    let (video_tx, mut video_rx) = mpsc::channel::<ClassifiedStream>(1);
    let (ctrl_tx, mut ctrl_rx) = mpsc::channel::<ClassifiedStream>(4);
    let last_accept: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
    let last_accept_for_accept = last_accept.clone();
    let mut accept_stop = stop_rx.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = accept_stop.changed() => break,
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, addr)) => {
                            if let Ok(mut slot) = last_accept_for_accept.lock() {
                                *slot = Some(Instant::now());
                            }
                            let video_tx = video_tx.clone();
                            let ctrl_tx = ctrl_tx.clone();
                            tokio::spawn(async move {
                                match classify_incoming(stream, addr).await {
                                    Ok(c) if session_policy::hello_is_control_plane(&c.hello.role) => {
                                        tracing::info!("control plane from {addr}");
                                        let _ = ctrl_tx.send(c).await;
                                    }
                                    Ok(c) => {
                                        let _ = video_tx.send(c).await;
                                    }
                                    Err(err) => {
                                        tracing::warn!("hello classify failed from {addr}: {err:#}");
                                    }
                                }
                            });
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

    // Accept is live before reverse/launch so the pad's first TCP SYN is
    // classified, not left in the kernel backlog during IddCx.
    let first_usb = open_usb_tunnel(
        adb_path.as_ref(),
        reverse_serial.as_deref(),
        listen_port,
        &status,
        false,
        true,
    )
    .await;
    reverse_serial = first_usb.serial;
    let mut usb_wait_detail = first_usb.wait_detail;

    let mut prepared_displays: Option<Vec<displays::DisplayInfo>> = None;
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
                prepared_displays = Some(list);
                set_status(&status, "准备虚拟屏", "虚拟屏已就绪，开始等待平板…");
            }
            lighting_host::share_flow::VirtualPrepareOutcome::Abort { reason } => {
                stop.store(true, Ordering::Relaxed);
                cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref(), listen_port).await;
                anyhow::bail!(
                    "{}",
                    lighting_host::share_flow::virtual_prepare_abort_message(&format!(
                        "{} [{reason}]",
                        lighting_host::ui_text::human_last_error(&reason)
                    ))
                );
            }
        }
    } else if let Err(err) = displays::apply_project_mode(req.share_mode) {
        tracing::warn!("DisplaySwitch failed ({err:#}); continuing with current layout");
    }

    // Honor IddCx bounces USB (parked Hellos are RST). Lenovo Xiaoxin Pad
    // 2020 keeps USB — a live Hello must not be dropped then `--remove`'d.
    let mut pending_hello: Option<ClassifiedStream> = None;
    if session_policy::refresh_usb_after_virtual_prepare() {
        if session_policy::keep_live_hello_after_virtual_prepare() {
            pending_hello = take_live_classified(&mut video_rx);
        } else if session_policy::drop_parked_hellos_after_virtual_prepare() {
            let video_n = drop_parked_classified(&mut video_rx);
            let ctrl_n = drop_parked_classified(&mut ctrl_rx);
            if video_n + ctrl_n > 0 {
                tracing::info!("dropped {video_n} stale video / {ctrl_n} control hellos after VDD");
            }
        }
        let last_accept_ms = last_accept
            .lock()
            .ok()
            .and_then(|g| *g)
            .map(|t| t.elapsed().as_millis() as u64);
        let force = session_policy::should_force_reverse_after_virtual_prepare(
            pending_hello.is_some(),
            last_accept_ms,
        );
        if force {
            if let Ok(mut slot) = last_accept.lock() {
                *slot = None;
            }
            let _ = drop_parked_classified(&mut ctrl_rx);
            let refreshed = open_usb_tunnel(
                adb_path.as_ref(),
                reverse_serial.as_deref().or(req.device_serial.as_deref()),
                listen_port,
                &status,
                true,
                true,
            )
            .await;
            reverse_serial = refreshed.serial;
            usb_wait_detail = refreshed.wait_detail;
            set_status(&status, "等待设备", usb_wait_detail.clone());
        } else if let Some(hello) = pending_hello.as_ref() {
            tracing::info!("keeping live Hello after VDD from {}", hello.addr);
            set_status(&status, "等待设备", "平板已连上，正在适配虚拟屏…");
        } else {
            let refreshed = open_usb_tunnel(
                adb_path.as_ref(),
                reverse_serial.as_deref().or(req.device_serial.as_deref()),
                listen_port,
                &status,
                false,
                false,
            )
            .await;
            reverse_serial = refreshed.serial;
            usb_wait_detail = refreshed.wait_detail;
            set_status(&status, "等待设备", usb_wait_detail.clone());
        }
    }

    // Do not sit on phase "启动" / 正在枚举显示器: that pins the UI on
    // 启用虚拟屏 while we are actually waiting for the pad. Reuse the
    // post-VDD list so CCD is not queried again while IddCx is settling.
    let displays = if let Some(list) = prepared_displays.filter(|l| !l.is_empty()) {
        list
    } else {
        tokio::task::spawn_blocking(displays::list_displays)
            .await
            .context("枚举显示器任务中断")??
    };
    req.display_index = displays::pick_display_index(&displays, req.share_mode)
        .context("所选投屏模式没有可用显示器，请刷新列表")?;
    set_status(&status, "等待设备", usb_wait_detail);
    let mut display = displays
        .get(req.display_index)
        .cloned()
        .context("所选显示器不存在，请刷新列表")?;
    let mut virtual_mode_applied = false;

    let mut stop_rx = stop_rx;
    let usb_refresh_busy = Arc::new(AtomicBool::new(false));
    let mut hello_wait = Instant::now();
    let mut last_reverse_recreate: Option<Instant> = None;
    loop {
        if !session_policy::continue_accept_loop(stop.load(Ordering::Relaxed)) {
            cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref(), listen_port).await;
            return Ok(());
        }

        let incoming = if let Some(c) = pending_hello.take() {
            if let Ok(mut st) = status.lock() {
                clear_peer_metrics(&mut st);
                st.client_addr = c.addr.to_string();
            }
            set_status(&status, "已连接", format!("{}", c.addr));
            c
        } else {
        tokio::select! {
            _ = stop_rx.changed() => {
                cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref(), listen_port).await;
                return Ok(());
            }
            incoming = video_rx.recv() => {
                let Some(c) = incoming else {
                    cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref(), listen_port).await;
                    anyhow::bail!("listen loop ended");
                };
                if let Ok(mut st) = status.lock() {
                    clear_peer_metrics(&mut st);
                    st.client_addr = c.addr.to_string();
                }
                set_status(&status, "已连接", format!("{}", c.addr));
                c
            }
            _ = tokio::time::sleep(Duration::from_millis(
                session_policy::usb_wait_refresh_ms(),
            )) => {
                if session_policy::refresh_usb_while_waiting_for_hello() {
                    if let (Some(adb_bin), Some(serial)) =
                        (adb_path.clone(), reverse_serial.clone())
                    {
                        if usb_refresh_busy
                            .compare_exchange(
                                false,
                                true,
                                Ordering::Relaxed,
                                Ordering::Relaxed,
                            )
                            .is_ok()
                        {
                            let busy = usb_refresh_busy.clone();
                            let status_ref = status.clone();
                            let last_accept_ms = last_accept
                                .lock()
                                .ok()
                                .and_then(|g| *g)
                                .map(|t| t.elapsed().as_millis() as u64);
                            let last_recreate_ms = last_reverse_recreate
                                .map(|t| t.elapsed().as_millis() as u64);
                            let force_stale = session_policy::should_force_stale_reverse(
                                hello_wait.elapsed().as_millis() as u64,
                                last_recreate_ms,
                                last_accept_ms,
                            );
                            if force_stale {
                                last_reverse_recreate = Some(Instant::now());
                                // Await on this task: a spawned --remove races
                                // the next Hello and parks the pad on 重连中.
                                match adb::recreate_reverse_port(
                                    &adb_bin,
                                    &serial,
                                    listen_port,
                                )
                                .await
                                {
                                    Ok(()) => {
                                        let n = drop_parked_classified(&mut video_rx)
                                            + drop_parked_classified(&mut ctrl_rx);
                                        if n > 0 {
                                            tracing::info!(
                                                "dropped {n} hellos from reverse --remove"
                                            );
                                        }
                                        if let Ok(mut slot) = last_accept.lock() {
                                            *slot = None;
                                        }
                                        set_transport(
                                            &status,
                                            format!("USB · adb reverse 已就绪（{serial}）"),
                                        );
                                    }
                                    Err(err) => {
                                        tracing::warn!(
                                            "wait-hello reverse refresh failed: {err:#}"
                                        );
                                    }
                                }
                                busy.store(false, Ordering::Relaxed);
                            } else {
                                tokio::spawn(async move {
                                    match adb::ensure_reverse_port(
                                        &adb_bin,
                                        &serial,
                                        listen_port,
                                    )
                                    .await
                                    {
                                        Ok(restored_missing) => {
                                            if restored_missing {
                                                set_transport(
                                                    &status_ref,
                                                    format!(
                                                        "USB · adb reverse 已就绪（{serial}）"
                                                    ),
                                                );
                                            }
                                            if session_policy::should_relaunch_client_after_reverse(
                                                restored_missing,
                                                false,
                                            ) {
                                                adb::launch_stream_client(
                                                    &adb_bin,
                                                    &serial,
                                                    listen_port,
                                                )
                                                .await;
                                            }
                                        }
                                        Err(err) => {
                                            tracing::warn!(
                                                "wait-hello reverse refresh failed: {err:#}"
                                            );
                                        }
                                    }
                                    busy.store(false, Ordering::Relaxed);
                                });
                            }
                        }
                    }
                }
                continue;
            }
        }
        };

        let outcome = handle_client(
            incoming,
            &mut ctrl_rx,
            &mut display,
            ffmpeg.clone(),
            req.clone(),
            status.clone(),
            stop.clone(),
            controls.clone(),
            tablet_only.clone(),
            preserve.clone(),
            &mut virtual_mode_applied,
        )
        .await;
        let wait_after_virtual = matches!(&outcome, Ok(ClientOutcome::NeedHelloAfterVirtualMode));
        match &outcome {
            Ok(ClientOutcome::NeedHelloAfterVirtualMode) => {
                tracing::info!("virtual mode applied; waiting for a fresh Hello (USB may re-enum)");
            }
            Ok(ClientOutcome::Finished) => {
                tracing::info!("client session ended");
            }
            Err(err) => {
                tracing::warn!("client session ended: {err:#}");
                let msg = format!("{err:#}");
                if !session_policy::continue_accept_after_handle_client_err(&msg) {
                    cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref(), listen_port)
                        .await;
                    anyhow::bail!("{msg}");
                }
            }
        }

        // Tablet sleep / drop: ffmpeg is already dead (handle_client waits).
        // "仅平板" must undo Win+P external *before* any SET_PRIMARY, otherwise
        // the internal panel stays detached and the GPU hangs until
        // Win+Ctrl+Shift+B. Do not CCD after a virtual-mode Hello: that
        // fights the size we just applied and bounces USB again.
        if !wait_after_virtual {
            let was_tablet_only = tablet_only.swap(false, Ordering::SeqCst);
            if let Some(snap) = preserve.clone() {
                let action = lighting_host::session_policy::client_drop_desktop_action(
                    was_tablet_only,
                    if was_tablet_only {
                        lighting_host::session_policy::PrimaryRestoreAction::SetPrimary
                    } else {
                        lighting_host::session_policy::PrimaryRestoreAction::TimingOnly
                    },
                );
                let _ = tokio::task::spawn_blocking(move || match action {
                    lighting_host::session_policy::ClientDropDesktopAction::UndoExternal => {
                        if let Err(err) = displays::restore_after_tablet_only(&snap) {
                            tracing::warn!("restore after tablet-only disconnect: {err:#}");
                        } else {
                            tracing::info!("restored laptop after tablet sleep/disconnect");
                        }
                    }
                    lighting_host::session_policy::ClientDropDesktopAction::ReassertPrimary => {
                        if let Err(err) = displays::reassert_primary(&snap) {
                            tracing::warn!("reassert primary after tablet drop: {err:#}");
                        }
                    }
                    lighting_host::session_policy::ClientDropDesktopAction::None => {}
                })
                .await;
            }
        }

        if !session_policy::continue_accept_loop(stop.load(Ordering::Relaxed)) {
            cleanup_reverse(adb_path.as_ref(), reverse_serial.as_deref(), listen_port).await;
            return Ok(());
        }

        // After IddCx mode change the Hello TCP is dead. Rebuild reverse
        // here (not in a spawn) so `--remove` cannot overlap the retry.
        // Parked Hellos from during --remove are half-open — drop them.
        if wait_after_virtual && session_policy::sync_recreate_reverse_after_virtual_mode() {
            if let Ok(mut slot) = last_accept.lock() {
                *slot = None;
            }
            tokio::time::sleep(Duration::from_millis(
                session_policy::virtual_mode_usb_settle_ms(),
            ))
            .await;
            if let (Some(adb_bin), Some(serial)) = (adb_path.clone(), reverse_serial.clone()) {
                set_status(&status, "等待设备", "虚拟屏已设为平板分辨率，正在恢复 USB…");
                match adb::recreate_reverse_port(&adb_bin, &serial, listen_port).await {
                    Ok(()) => {
                        set_transport(
                            &status,
                            format!("USB · adb reverse 已就绪（{serial}）"),
                        );
                        if session_policy::relaunch_client_after_virtual_mode_reverse() {
                            adb::launch_stream_client(&adb_bin, &serial, listen_port).await;
                        }
                    }
                    Err(err) => {
                        tracing::warn!("re-apply adb reverse after virtual mode: {err:#}");
                    }
                }
            }
            let n = drop_parked_classified(&mut video_rx) + drop_parked_classified(&mut ctrl_rx);
            if n > 0 {
                tracing::info!("dropped {n} hellos from virtual-mode reverse rebuild");
            }
            last_reverse_recreate = Some(Instant::now());
        } else if session_policy::background_recreate_reverse_after_client() {
            if let (Some(adb_bin), Some(serial)) = (adb_path.clone(), reverse_serial.clone()) {
                tokio::spawn(async move {
                    if let Err(err) =
                        adb::recreate_reverse_port(&adb_bin, &serial, listen_port).await
                    {
                        tracing::warn!("re-apply adb reverse failed: {err:#}");
                    }
                });
            }
        } else if let (Some(adb_bin), Some(serial)) = (adb_path.clone(), reverse_serial.clone()) {
            tokio::spawn(async move {
                if let Err(err) = adb::ensure_reverse_port(&adb_bin, &serial, listen_port).await
                {
                    tracing::warn!("ensure adb reverse after drop failed: {err:#}");
                }
            });
        }
        if let Ok(mut st) = status.lock() {
            clear_peer_metrics(&mut st);
        }
        hello_wait = Instant::now();
        if !wait_after_virtual {
            last_reverse_recreate = None;
        }
        if wait_after_virtual {
            set_status(&status, "等待设备", "虚拟屏已设为平板分辨率，等待重新连接");
        } else {
            set_status(&status, "等待设备", "上一台已断开，等待重新连接");
        }
    }
}

struct UsbTunnel {
    serial: Option<String>,
    wait_detail: String,
}

fn drop_parked_classified(rx: &mut mpsc::Receiver<ClassifiedStream>) -> usize {
    let mut n = 0;
    while rx.try_recv().is_ok() {
        n += 1;
    }
    n
}

/// Bind is already listening. Reverse 127.0.0.1:port and open the pad app.
/// Called before VDD (so the 90s USB retry can start) and again after VDD
/// (USB often re-enumerates during IddCx / UAC).
fn tcp_half_dead(reader: &tokio::net::tcp::OwnedReadHalf) -> bool {
    let mut buf = [0u8; 1];
    match reader.try_read(&mut buf) {
        Ok(0) => true,
        Ok(_) => false,
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(_) => true,
    }
}

fn take_live_classified(rx: &mut mpsc::Receiver<ClassifiedStream>) -> Option<ClassifiedStream> {
    let mut live = None;
    while let Ok(c) = rx.try_recv() {
        if tcp_half_dead(&c.reader) {
            tracing::info!("dropped dead parked hello from {}", c.addr);
            continue;
        }
        if live.is_none() {
            live = Some(c);
        }
    }
    live
}

async fn open_usb_tunnel(
    adb_path: Option<&std::path::PathBuf>,
    preferred_serial: Option<&str>,
    listen_port: u16,
    status: &Arc<Mutex<SessionStatus>>,
    force_recreate: bool,
    launch: bool,
) -> UsbTunnel {
    let Some(adb_bin) = adb_path else {
        let wait_detail = "未找到 adb，平板可填电脑 IP 用 Wi-Fi 测试".to_string();
        set_transport(status, "未找到 adb · 仅局域网可用（平板填电脑 IP）");
        set_status(status, "等待设备", wait_detail.clone());
        return UsbTunnel {
            serial: None,
            wait_detail,
        };
    };
    let serial = if let Some(s) = preferred_serial.filter(|s| !s.is_empty()) {
        s.to_string()
    } else {
        match adb::list_ready_serials(adb_bin)
            .await
            .unwrap_or_default()
            .into_iter()
            .next()
        {
            Some(s) => s,
            None => {
                let wait_detail = "未检测到已授权设备，平板可填电脑 IP".to_string();
                set_transport(status, "USB · 未检测到已授权设备，可走 Wi-Fi（填电脑 IP）");
                set_status(status, "等待设备", wait_detail.clone());
                return UsbTunnel {
                    serial: None,
                    wait_detail,
                };
            }
        }
    };
    set_status(
        status,
        "等待设备",
        format!("正在执行 adb reverse（{serial}）"),
    );
    let reverse_result = if force_recreate {
        adb::recreate_reverse_port(adb_bin, &serial, listen_port).await
    } else {
        adb::reverse_port(adb_bin, &serial, listen_port).await
    };
    if let Err(err) = reverse_result {
        let wait_detail = format!("adb reverse 失败，仍可走局域网：{err:#}");
        set_transport(
            status,
            "USB · adb reverse 失败，可改用 Wi-Fi（平板填电脑 IP）",
        );
        set_status(status, "等待设备", wait_detail.clone());
        return UsbTunnel {
            serial: Some(serial),
            wait_detail,
        };
    }
    set_transport(status, format!("USB · adb reverse 已就绪（{serial}）"));
    if launch {
        if session_policy::launch_stream_client_does_not_block_listen() {
            let adb_owned = adb_bin.clone();
            let serial_owned = serial.clone();
            tokio::spawn(async move {
                adb::launch_stream_client(&adb_owned, &serial_owned, listen_port).await;
            });
        } else {
            adb::launch_stream_client(adb_bin, &serial, listen_port).await;
        }
    }
    let wait_detail = format!("USB 已就绪（{serial}），正在打开平板投屏");
    set_status(status, "等待设备", wait_detail.clone());
    UsbTunnel {
        serial: Some(serial),
        wait_detail,
    }
}

async fn cleanup_reverse(adb: Option<&std::path::PathBuf>, serial: Option<&str>, port: u16) {
    if let (Some(adb), Some(serial)) = (adb, serial) {
        let _ = adb::remove_reverse(adb, serial, port).await;
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

#[allow(clippy::too_many_arguments)]
async fn handle_client(
    incoming: ClassifiedStream,
    ctrl_rx: &mut mpsc::Receiver<ClassifiedStream>,
    display: &mut DisplayInfo,
    ffmpeg: std::path::PathBuf,
    req: SessionRequest,
    status: Arc<Mutex<SessionStatus>>,
    stop: Arc<AtomicBool>,
    controls: Arc<Controls>,
    tablet_only: Arc<AtomicBool>,
    preserve: Option<displays::PrimarySnapshot>,
    virtual_mode_applied: &mut bool,
) -> Result<ClientOutcome> {
    let ClassifiedStream {
        mut reader,
        mut writer,
        hello,
        addr,
    } = incoming;
    tracing::info!("hello from {addr}: {:?}", hello);
    if let Ok(mut s) = status.lock() {
        s.client_name = hello.device.trim().to_string();
    }

    let mut mode_guard = displays::ModeRestoreGuard(None);
    let codec = pick_codec(&hello, req.prefer_hevc);
    let (dec_w, dec_h, dec_fps, hw) = codec_limit(&hello, &codec);
    let scale = if req.match_device || req.share_mode.uses_virtual_display() {
        req.scale
    } else {
        1.0
    };
    let align = hello.alignment.max(2);
    // Size IddCx to the encoder output (alignment + quality scale), not the
    // raw Hello panel. Mismatch used to force scale_d3d11's 10-frame pool.
    let (panel_w, panel_h) = lighting_host::session_policy::virtual_panel_size(
        hello.screen_width,
        hello.screen_height,
        req.max_width,
        req.max_height,
        if req.share_mode.uses_virtual_display() {
            scale
        } else {
            1.0
        },
        dec_w,
        dec_h,
        align,
    );
    let mut after_virtual_mode_change = false;
    if req.share_mode.uses_virtual_display() && hello.screen_width > 0 && hello.screen_height > 0 {
        // Independent second screen: virtual monitor = encode size so
        // capture is 1:1 (no scaling anywhere) and the PC monitor is untouched.
        // Mode change still resets DDA; encoder start waits dda_settle and
        // Stop is polled. Skipping this left 16:9 VDD letterboxed on 16:10.
        if !session_policy::resize_virtual_display_after_hello() {
            *virtual_mode_applied = true;
            set_status(
                &status,
                "独立第二屏",
                format!(
                    "按当前虚拟屏 {}×{} 推流",
                    display.width, display.height
                ),
            );
        } else if *virtual_mode_applied {
            set_status(
                &status,
                "独立第二屏",
                format!(
                    "虚拟屏已是平板分辨率 {}×{}，开始推流",
                    display.width, display.height
                ),
            );
        } else {
            set_status(
                &status,
                "独立第二屏",
                format!("正在把虚拟屏设为编码分辨率 {panel_w}×{panel_h}"),
            );
            let (tw, th) = (panel_w, panel_h);
            let want_fps = hello.max_fps.max(24).min(120);
            let preserve_for_mode = preserve.clone();
            match tokio::task::spawn_blocking(move || {
                displays::configure_virtual_for_tablet(tw, th, want_fps, preserve_for_mode.as_ref())
            })
            .await
            {
                Ok(Ok((updated, changed))) => {
                    tracing::info!(
                        "virtual display now {}×{} changed={changed} (capture {:?})",
                        updated.width,
                        updated.height,
                        updated.dxgi
                    );
                    *display = updated;
                    *virtual_mode_applied = true;
                    after_virtual_mode_change = changed;
                    set_status(
                        &status,
                        "独立第二屏",
                        format!("虚拟屏 {}×{} · 1:1 抓取", display.width, display.height),
                    );
                    if session_policy::should_abandon_hello_after_virtual_mode(
                        changed,
                        tcp_half_dead(&reader),
                    ) {
                        while ctrl_rx.try_recv().is_ok() {}
                        set_status(&status, "等待设备", "虚拟屏已设为平板分辨率，正在恢复 USB…");
                        return Ok(ClientOutcome::NeedHelloAfterVirtualMode);
                    }
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
        }
    } else if req.match_device && hello.screen_width > 0 && hello.screen_height > 0 {
        // Mirror + 跟随平板. Do this once per share: switching the PC
        // panel on every Hello resets DXGI and loops 适配分辨率 / 已断开.
        if *virtual_mode_applied {
            set_status(
                &status,
                "适配平板",
                format!("按平板分辨率编码 {}×{}", hello.screen_width, hello.screen_height),
            );
        } else {
        let (tw, th) = lighting_host::session_policy::orient_box(
            display.width,
            display.height,
            hello.screen_width,
            hello.screen_height,
        );
        let prefer_fps = hello.max_fps.max(30).min(60);
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
                    after_virtual_mode_change = true;
                }
                let device_name = display.name.clone();
                if let Ok(list) = displays::list_displays() {
                    if let Some(updated) = list.iter().find(|d| d.name == device_name).cloned() {
                        *display = updated;
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
        *virtual_mode_applied = true;
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
                // Win+P /external can reset the virtual mode to 30 Hz. Put 60+
                // back and refresh DXGI *after* the topology change.
                let (tw, th) = (panel_w, panel_h);
                let want_fps = hello.max_fps.max(24).min(120);
                let preserve_for_hz = preserve.clone();
                if tw > 0 && th > 0 {
                    match tokio::task::spawn_blocking(move || {
                        displays::configure_virtual_for_tablet(
                            tw,
                            th,
                            want_fps,
                            preserve_for_hz.as_ref(),
                        )
                    })
                    .await
                    {
                        Ok(Ok((updated, changed))) => {
                            *display = updated;
                            after_virtual_mode_change = after_virtual_mode_change || changed;
                        }
                        Ok(Err(err)) => {
                            tracing::warn!("reapply virtual Hz after tablet-only: {err:#}")
                        }
                        Err(err) => tracing::warn!("reapply virtual Hz join: {err:#}"),
                    }
                }
                let list = displays::list_displays()?;
                if let Some(updated) = list.into_iter().find(|d| d.name == display.name) {
                    *display = updated;
                }
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

    // Virtual: encode the desktop IddCx actually landed on when that size
    // still fits the decoder. Re-clamping to Hello.alignment (16) used to
    // turn 1920×1080 into 1920×1072 (not in the VDD table) and force
    // ffmpeg scale_d3d11's 10-frame pool. Mirror still clamps to the pad.
    let (width, height) = if req.share_mode.uses_virtual_display() {
        lighting_host::session_policy::encode_keep_dda_identity(
            display.width,
            display.height,
            panel_w,
            panel_h,
            dec_w,
            dec_h,
        )
    } else {
        let (w, h) = lighting_host::session_policy::compute_encode_size(
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
        (
            lighting_host::session_policy::align_dim(w, align),
            lighting_host::session_policy::align_dim(h, align),
        )
    };

    let fps = adapted_fps(req.fps, hello.max_fps, dec_fps, hw);
    let auto_br = auto_bitrate(width, height, fps);
    // Honor the user's bitrate for hardware decode — auto_br used to silently
    // cap 1080p@60 to ~15 Mbps even when the slider said 25 Mbps.
    let bitrate_kbps = if hw {
        req.bitrate_kbps
            .clamp(4_000, 80_000)
            .max(auto_br.min(req.bitrate_kbps))
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
    let settings = EncodeSettings {
        width,
        height,
        fps,
        bitrate_kbps: cfg.bitrate_kbps,
        codec: codec.clone(),
        profile: if hw || codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265") {
            "main".into()
        } else {
            "baseline".into()
        },
        draw_mouse: !hello.cursor_overlay,
        nvenc_surfaces: lighting_host::session_policy::nvenc_surfaces(),
        nvenc_rc: lighting_host::session_policy::nvenc_rc().into(),
        amf_rc: lighting_host::session_policy::amf_rc().into(),
    };
    // ffmpeg must emit codec-config+IDR *before* CONFIG. Sending CONFIG first
    // parked the tablet on "avc … 等待关键帧" for the whole graph bootstrap,
    // then reconnect if the pipe died before IDR.
    let hevc = codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265");
    set_status(&status, "编码", "正在启动抓屏…");
    let (session, bootstrap, capture_kind) = match start_live_encoder_resilient(
        &ffmpeg,
        display,
        &settings,
        hevc,
        after_virtual_mode_change,
        &status,
        &stop,
    )
    .await
    {
        Ok(v) => v,
        Err(_) if stop.load(Ordering::Relaxed) => {
            return Ok(ClientOutcome::Finished);
        }
        Err(err) => return Err(err),
    };
    let dda_retries = 0u8;

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

    let cursor_slot: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let cursor_stop = std::sync::Arc::new(AtomicBool::new(false));
    let cursor_notify = Arc::new(Notify::new());
    if hello.cursor_overlay {
        crate::cursor::spawn_sampler(
            display.clone(),
            cursor_slot.clone(),
            cursor_stop.clone(),
            cursor_notify.clone(),
        );
        tracing::info!("tablet cursor overlay on; video will not bake the OS pointer");
    }
    let display_for_input = display.clone();
    let (touch_tx, touch_rx) = std::sync::mpsc::channel::<protocol::TouchEvent>();
    let input_display = display_for_input.clone();
    std::thread::Builder::new()
        .name("lighting-input".into())
        .spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Threading::SetThreadPriority(
                    windows::Win32::System::Threading::GetCurrentThread(),
                    windows::Win32::System::Threading::THREAD_PRIORITY_HIGHEST,
                );
            }
            while let Ok(ev) = touch_rx.recv() {
                input::inject_touch(&input_display, ev);
            }
        })
        .ok();
    let touch_for_control = touch_tx.clone();
    let mut control_task = attach_control_plane(
        ctrl_rx,
        cursor_slot.clone(),
        cursor_notify.clone(),
        cursor_stop.clone(),
        touch_for_control.clone(),
        controls.clone(),
        false,
    )
    .await;
    let mux_cursor_on_video =
        session_policy::mux_cursor_on_video(hello.cursor_overlay, control_task.is_some());

    if capture_kind == CaptureKind::Gdi {
        tracing::warn!("using gdigrab; games will look like 10–20 fps");
        set_status(
            &status,
            "编码",
            "当前是 GDI 抓屏，游戏会明显卡。若本机有独显，请把虚拟屏绑到独显后重试。",
        );
    }
    let t0 = std::time::Instant::now();
    let audio_stop = Arc::new(AtomicBool::new(false));
    let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel::<crate::audio::AudioPacket>(
        lighting_host::session_policy::audio_capture_queue(),
    );
    if audio_enabled {
        match crate::audio::start_loopback(audio_tx, audio_stop.clone(), t0) {
            Ok(()) => tracing::info!("audio loopback started"),
            Err(err) => {
                tracing::warn!("audio loopback unavailable: {err:#}");
            }
        }
    }

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
                            tracing::debug!(
                                "host got touch action={} x={} y={}",
                                ev.action,
                                ev.x,
                                ev.y
                            );
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

    // Dedicated write thread sits in recv() so a ready AU is not parked
    // until tokio's blocking pool picks up spawn_blocking. Control attach
    // stays on this async task (ctrl_rx is borrowed across reconnects).
    let mux_cursor = Arc::new(AtomicBool::new(mux_cursor_on_video));
    let (done_tx, mut done_rx) = oneshot::channel::<encoder::EncoderSession>();
    let rt = tokio::runtime::Handle::current();
    let stop_w = stop.clone();
    let mux_w = mux_cursor.clone();
    let slot_w = cursor_slot.clone();
    let status_w = status.clone();
    let ping_w = ping_sent.clone();
    let ffmpeg_w = ffmpeg.clone();
    let display_w = display.clone();
    let settings_w = settings.clone();
    std::thread::Builder::new()
        .name("lighting-video-write".into())
        .spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Threading::SetThreadPriority(
                    windows::Win32::System::Threading::GetCurrentThread(),
                    windows::Win32::System::Threading::THREAD_PRIORITY_HIGHEST,
                );
            }
            lighting_host::annexb::enter_mmcss();
            let session = video_write_loop(
                rt,
                writer,
                session,
                capture_kind,
                dda_retries,
                stop_w,
                mux_w,
                slot_w,
                audio_rx,
                status_w,
                ping_w,
                t0,
                ffmpeg_w,
                display_w,
                settings_w,
                hevc,
            );
            let _ = done_tx.send(session);
        })
        .context("video write thread")?;

    let mut wait_control = control_task.is_none();
    let mut session = loop {
        tokio::select! {
            biased;
            back = &mut done_rx => {
                break match back {
                    Ok(s) => s,
                    Err(_) => anyhow::bail!("video write thread panicked"),
                };
            }
            ctrl = ctrl_rx.recv(), if wait_control => {
                match ctrl {
                    Some(ctrl) => {
                        control_task = Some(bind_control(
                            ctrl,
                            cursor_slot.clone(),
                            cursor_notify.clone(),
                            cursor_stop.clone(),
                            touch_for_control.clone(),
                            controls.clone(),
                        ));
                        mux_cursor.store(false, Ordering::Relaxed);
                        wait_control = false;
                    }
                    None => wait_control = false,
                }
            }
        }
    };

    cursor_stop.store(true, Ordering::Relaxed);
    audio_stop.store(true, Ordering::Relaxed);
    if let Some(task) = control_task.take() {
        task.abort();
    }
    while ctrl_rx.try_recv().is_ok() {}
    reader_task.abort();
    // Must wait for ffmpeg to release DXGI before any topology restore.
    // Killing it in the background and immediately CCD-ing is the GPU hang
    // that only Win+Ctrl+Shift+B could clear after the tablet slept.
    session.stop();
    tokio::time::sleep(Duration::from_millis(200)).await;
    drop(mode_guard);
    Ok(ClientOutcome::Finished)
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

fn encode_video_packet(t0: std::time::Instant, pkt: &EncodedPacket) -> Vec<u8> {
    let payload = protocol::with_pts(t0.elapsed().as_micros() as u64, &pkt.data);
    session_policy::lit1_encode(protocol::MSG_VIDEO, video_flags(pkt), &payload)
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

/// Sit in `recv_timeout` on this thread. spawn_blocking-per-picture used to
/// leave a ready AU parked until the blocking pool ran — one scheduler
/// slice vs the laptop, which GlideX does not pay. Writes still hop to the
/// runtime via `Handle::block_on` so control-plane attach can keep `ctrl_rx`.
#[allow(clippy::too_many_arguments)]
fn video_write_loop(
    handle: tokio::runtime::Handle,
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    mut session: encoder::EncoderSession,
    mut capture_kind: CaptureKind,
    mut dda_retries: u8,
    stop: Arc<AtomicBool>,
    mux_cursor: Arc<AtomicBool>,
    cursor_slot: Arc<Mutex<Option<Vec<u8>>>>,
    audio_rx: std::sync::mpsc::Receiver<crate::audio::AudioPacket>,
    status: Arc<Mutex<SessionStatus>>,
    ping_sent: Arc<Mutex<Option<std::time::Instant>>>,
    t0: std::time::Instant,
    ffmpeg: std::path::PathBuf,
    display: DisplayInfo,
    settings: EncodeSettings,
    hevc: bool,
) -> encoder::EncoderSession {
    let mut last_ping = std::time::Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let mux = mux_cursor.load(Ordering::Relaxed);
        if mux {
            let cursor_payload = cursor_slot.lock().ok().and_then(|mut slot| slot.take());
            if let Some(payload) = cursor_payload {
                if handle
                    .block_on(protocol::write_message(
                        &mut writer,
                        protocol::MSG_CURSOR,
                        0,
                        &payload,
                    ))
                    .is_err()
                {
                    break;
                }
            }
        }
        if last_ping.elapsed() >= Duration::from_millis(1_000) {
            last_ping = std::time::Instant::now();
            if handle
                .block_on(protocol::write_message(
                    &mut writer,
                    protocol::MSG_HEARTBEAT,
                    0,
                    &[],
                ))
                .is_ok()
            {
                if let Ok(mut slot) = ping_sent.lock() {
                    slot.get_or_insert_with(std::time::Instant::now);
                }
                if let Ok(mut s) = status.lock() {
                    s.connected_secs = t0.elapsed().as_secs();
                }
            }
        }
        let tick = if mux {
            Duration::from_millis(1)
        } else if session_policy::audio_flush_on_video_timeout() {
            Duration::from_millis(10)
        } else {
            Duration::from_millis(1_000)
        };
        match session.rx.recv_timeout(tick) {
            Ok(pkt) => {
                let mut out = encode_video_packet(t0, &pkt);
                if session_policy::coalesce_extra_video_on_write() {
                    while let Ok(more) = session.rx.try_recv() {
                        out.extend_from_slice(&encode_video_packet(t0, &more));
                    }
                }
                append_pending_audio(&mut out, &audio_rx);
                let sent = out.len();
                if handle.block_on(writer.write_all(&out)).is_err() {
                    break;
                }
                if handle.block_on(writer.flush()).is_err() {
                    break;
                }
                if let Ok(mut s) = status.lock() {
                    s.frames += 1;
                    s.bytes_sent += sent as u64;
                    s.connected_secs = t0.elapsed().as_secs();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if session_policy::audio_flush_on_video_timeout() {
                    let mut out = Vec::new();
                    append_pending_audio(&mut out, &audio_rx);
                    if !out.is_empty() {
                        if handle.block_on(writer.write_all(&out)).is_err() {
                            break;
                        }
                        if handle.block_on(writer.flush()).is_err() {
                            break;
                        }
                    }
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                session.stop();
                let retry_dda = capture_kind == CaptureKind::Dda && dda_retries < 1;
                dda_retries = dda_retries.saturating_add(1);
                if retry_dda {
                    tracing::warn!("encoder pipe closed, retrying Desktop Duplication");
                    set_status(&status, "抓屏恢复", "Desktop Duplication 中断，正在重连…");
                } else {
                    tracing::warn!("encoder pipe closed, falling back to gdigrab");
                    set_status(&status, "回退", "改用 gdigrab 抓屏");
                }
                let restarted = if retry_dda {
                    handle.block_on(start_live_encoder(
                        &ffmpeg, &display, &settings, hevc, &stop,
                    ))
                } else {
                    handle
                        .block_on(restart_encoder_with_bootstrap(
                            &ffmpeg, &display, &settings, hevc, &stop,
                        ))
                        .map(|(s, b)| (s, b, CaptureKind::Gdi))
                };
                match restarted {
                    Ok((new_session, bootstrap, kind)) => {
                        session = new_session;
                        capture_kind = kind;
                        match handle.block_on(write_bootstrap(&mut writer, t0, &bootstrap)) {
                            Ok(sent) => add_wire_bytes(&status, sent),
                            Err(err) => {
                                tracing::warn!(
                                    "send bootstrap after encoder restart failed: {err:#}"
                                );
                                break;
                            }
                        }
                        set_status(
                            &status,
                            "编码",
                            if kind == CaptureKind::Dda {
                                "Desktop Duplication 已重发 codec-config + IDR"
                            } else {
                                "gdigrab 已重发 codec-config + IDR"
                            },
                        );
                    }
                    Err(err) => {
                        tracing::warn!("encoder restart failed: {err:#}");
                        set_status(&status, "错误", "抓屏重启失败，请查看日志");
                        break;
                    }
                }
            }
        }
    }
    let _ = handle.block_on(writer.shutdown());
    session
}

async fn wait_encoder_bootstrap(
    session: encoder::EncoderSession,
    hevc: bool,
    stop: &Arc<AtomicBool>,
) -> Result<(encoder::EncoderSession, Vec<EncodedPacket>)> {
    let stop = stop.clone();
    tokio::task::spawn_blocking(move || {
        let pkts = annexb::recv_bootstrap_until(&session.rx, Duration::from_secs(3), hevc, || {
            stop.load(Ordering::Relaxed)
        });
        (session, pkts)
    })
    .await
    .context("encoder bootstrap worker")
    .and_then(|(session, pkts)| Ok((session, pkts?)))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureKind {
    Dda,
    Gdi,
}

async fn start_live_encoder_resilient(
    ffmpeg: &std::path::PathBuf,
    display: &mut DisplayInfo,
    settings: &EncodeSettings,
    hevc: bool,
    after_mode_change: bool,
    status: &Arc<Mutex<SessionStatus>>,
    stop: &Arc<AtomicBool>,
) -> Result<(encoder::EncoderSession, Vec<EncodedPacket>, CaptureKind)> {
    let attempts = if after_mode_change {
        lighting_host::session_policy::encoder_start_attempts_after_mode_change()
    } else {
        lighting_host::session_policy::encoder_start_attempts()
    }
    .max(1);
    let settle = Duration::from_millis(
        lighting_host::session_policy::dda_settle_after_mode_change_ms(),
    );
    let mut last_err: Option<anyhow::Error> = None;
    for i in 0..attempts {
        if stop.load(Ordering::Relaxed) {
            anyhow::bail!("已停止");
        }
        if after_mode_change || i > 0 {
            tokio::time::sleep(settle).await;
            if let Ok(Ok(list)) = tokio::task::spawn_blocking(displays::list_displays).await {
                if let Some(updated) = list.into_iter().find(|d| d.name == display.name) {
                    *display = updated;
                }
            }
        }
        match start_live_encoder(ffmpeg, display, settings, hevc, stop).await {
            Ok(v) => return Ok(v),
            Err(err) => {
                tracing::warn!(
                    "encoder start attempt {}/{attempts} failed: {err:#}",
                    i + 1
                );
                if stop.load(Ordering::Relaxed) {
                    anyhow::bail!("已停止");
                }
                set_status(
                    status,
                    "编码",
                    format!("抓屏未就绪，正在重试 ({}/{})", i + 1, attempts),
                );
                last_err = Some(err);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("抓屏启动失败")))
}

async fn start_live_encoder(
    ffmpeg: &std::path::PathBuf,
    display: &DisplayInfo,
    settings: &EncodeSettings,
    hevc: bool,
    stop: &Arc<AtomicBool>,
) -> Result<(encoder::EncoderSession, Vec<EncodedPacket>, CaptureKind)> {
    if stop.load(Ordering::Relaxed) {
        anyhow::bail!("已停止");
    }
    if lighting_host::session_policy::prefer_gdigrab_capture(
        display.is_virtual,
        display.dxgi.is_some(),
    ) {
        match restart_encoder_with_bootstrap(ffmpeg, display, settings, hevc, stop).await {
            Ok((session, bootstrap)) => return Ok((session, bootstrap, CaptureKind::Gdi)),
            Err(err) => tracing::warn!("gdigrab-first for GDI-only display failed: {err:#}"),
        }
    }
    let mut last_err: Option<anyhow::Error> = None;
    let vendor = display.dxgi.map(|d| d.vendor_id).unwrap_or(0);
    for enc in encoder::encoder_fallback_chain_for(&settings.codec, vendor) {
        let graphs = lighting_host::capture_graph::dda_capture_graphs_for(
            display.dxgi,
            settings.fps,
            display.width,
            display.height,
            settings.width,
            settings.height,
            enc,
            settings.draw_mouse,
        );
        let surface_tries = if enc.contains("nvenc") {
            lighting_host::session_policy::nvenc_surface_attempts()
        } else {
            vec![settings.nvenc_surfaces.max(1)]
        };
        let rc_tries: Vec<String> = if enc.contains("nvenc") {
            lighting_host::session_policy::nvenc_rc_attempts()
                .into_iter()
                .map(|s| s.to_string())
                .collect()
        } else if enc.contains("amf") {
            lighting_host::session_policy::amf_rc_attempts()
                .into_iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            vec![String::new()]
        };
        for graph in graphs {
            for surfaces in &surface_tries {
                for rc in &rc_tries {
                    if stop.load(Ordering::Relaxed) {
                        anyhow::bail!("已停止");
                    }
                    let mut attempt = settings.clone();
                    attempt.nvenc_surfaces = *surfaces;
                    attempt.nvenc_rc = rc.clone();
                    if enc.contains("amf") && !rc.is_empty() {
                        attempt.amf_rc = rc.clone();
                    }
                    let session =
                        match encoder::start_encoder(ffmpeg, display, &attempt, enc, &graph) {
                            Ok(s) => s,
                            Err(err) => {
                                tracing::warn!("encoder {enc} spawn failed: {err:#}");
                                last_err = Some(err);
                                continue;
                            }
                        };
                    match wait_encoder_bootstrap(session, hevc, stop).await {
                        Ok((session, bootstrap)) => {
                            let virtual_output = display.is_virtual;
                            tracing::info!(
                            "using encoder {enc} graph={graph} surfaces={surfaces} rc={rc} (dda virtual={virtual_output})"
                        );
                            return Ok((session, bootstrap, CaptureKind::Dda));
                        }
                        Err(err) => {
                            tracing::warn!(
                            "{enc} graph died before codec-config + IDR ({graph} surfaces={surfaces} rc={rc}): {err:#}"
                        );
                            last_err = Some(err);
                        }
                    }
                }
            }
        }
    }
    tracing::warn!(
        "desktop duplication encoders failed, trying gdigrab: {:?}",
        last_err
    );
    restart_encoder_with_bootstrap(ffmpeg, display, settings, hevc, stop)
        .await
        .map(|(session, bootstrap)| (session, bootstrap, CaptureKind::Gdi))
}

async fn restart_encoder_with_bootstrap(
    ffmpeg: &std::path::PathBuf,
    display: &DisplayInfo,
    settings: &EncodeSettings,
    hevc: bool,
    stop: &Arc<AtomicBool>,
) -> Result<(encoder::EncoderSession, Vec<EncodedPacket>)> {
    let mut last_err: Option<anyhow::Error> = None;
    let vendor = display.dxgi.map(|d| d.vendor_id).unwrap_or(0);
    for enc in encoder::encoder_fallback_chain_for(&settings.codec, vendor) {
        let surface_tries = if enc.contains("nvenc") {
            lighting_host::session_policy::nvenc_surface_attempts()
        } else {
            vec![settings.nvenc_surfaces.max(1)]
        };
        let rc_tries: Vec<String> = if enc.contains("nvenc") {
            lighting_host::session_policy::nvenc_rc_attempts()
                .into_iter()
                .map(|s| s.to_string())
                .collect()
        } else if enc.contains("amf") {
            lighting_host::session_policy::amf_rc_attempts()
                .into_iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            vec![String::new()]
        };
        for surfaces in surface_tries {
            for rc in &rc_tries {
                if stop.load(Ordering::Relaxed) {
                    anyhow::bail!("已停止");
                }
                let mut attempt = settings.clone();
                attempt.nvenc_surfaces = surfaces;
                attempt.nvenc_rc = rc.clone();
                if enc.contains("amf") && !rc.is_empty() {
                    attempt.amf_rc = rc.clone();
                }
                let session = match encoder::start_encoder_gdigrab(ffmpeg, display, &attempt, enc) {
                    Ok(s) => s,
                    Err(err) => {
                        tracing::warn!("gdigrab encoder {enc} failed: {err:#}");
                        last_err = Some(err);
                        continue;
                    }
                };
                match wait_encoder_bootstrap(session, hevc, stop).await {
                    Ok((session, bootstrap)) => {
                        tracing::info!(
                            "gdigrab bootstrap ok with {enc} surfaces={surfaces} rc={rc}"
                        );
                        return Ok((session, bootstrap));
                    }
                    Err(err) => {
                        tracing::warn!("{enc} closed before codec-config + IDR (surfaces={surfaces} rc={rc}): {err:#}");
                        last_err = Some(err);
                    }
                }
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
    let screen_area =
        (hello.screen_width.max(1) as u64).saturating_mul(hello.screen_height.max(1) as u64);
    let avc_area = hello
        .avc_limit
        .as_ref()
        .map(|l| (l.width.max(1) as u64).saturating_mul(l.height.max(1) as u64))
        .unwrap_or_else(|| {
            (hello.decoder_max_width.max(1) as u64)
                .saturating_mul(hello.decoder_max_height.max(1) as u64)
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
    session_policy::encode_fps(req_fps, hello_max, dec_fps, hw)
}

#[cfg(test)]
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
            cursor_overlay: false,
            role: String::new(),
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
