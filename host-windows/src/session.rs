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
}

#[derive(Clone, Default)]
pub struct SessionStatus {
    pub running: bool,
    pub phase: String,
    pub detail: String,
    pub frames: u64,
}

pub async fn run_session(
    req: SessionRequest,
    status: Arc<Mutex<SessionStatus>>,
    stop: Arc<AtomicBool>,
) {
    let result = run_session_inner(req, status.clone(), stop).await;
    if let Err(err) = result {
        tracing::error!("{err:#}");
        if let Ok(mut s) = status.lock() {
            s.running = false;
            s.phase = "错误".into();
            s.detail = format!("{err:#}");
        }
    } else if let Ok(mut s) = status.lock() {
        s.running = false;
        if s.phase != "错误" {
            s.phase = "已停止".into();
        }
    }
}

fn set_status(status: &Arc<Mutex<SessionStatus>>, phase: &str, detail: impl Into<String>) {
    if let Ok(mut s) = status.lock() {
        s.running = true;
        s.phase = phase.into();
        s.detail = detail.into();
    }
}

async fn run_session_inner(
    req: SessionRequest,
    status: Arc<Mutex<SessionStatus>>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    set_status(&status, "启动", "正在枚举显示器");
    let displays = displays::list_displays()?;
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

    let adb_path = adb::find_adb().ok();
    let mut reverse_serial: Option<String> = None;
    if let Some(adb_bin) = adb_path.as_ref() {
        let devices = adb::list_devices(adb_bin).await.unwrap_or_default();
        let serial = req.device_serial.clone().or_else(|| {
            devices
                .into_iter()
                .find(|d| d.state == "device")
                .map(|d| d.serial)
        });
        if let Some(serial) = serial {
            set_status(&status, "USB", format!("adb reverse {serial}"));
            if let Err(err) = adb::reverse_port(adb_bin, &serial, protocol::PORT).await {
                set_status(
                    &status,
                    "USB 警告",
                    format!("adb reverse 失败（仍可走局域网）: {err:#}"),
                );
            } else {
                reverse_serial = Some(serial);
            }
        } else {
            set_status(
                &status,
                "等待",
                "未检测到已授权的 Android 设备，可改用局域网 IP",
            );
        }
    } else {
        set_status(
            &status,
            "等待",
            "未找到 adb，USB 不可用。平板可填电脑 IP 用 Wi-Fi 测试",
        );
    }

    set_status(&status, "等待设备", "请在平板上打开 Lighting 并连接");

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
        set_status(&status, "等待设备", "上一台已断开，等待重新连接");
    }
}

async fn cleanup_reverse(adb: Option<&std::path::PathBuf>, serial: Option<&str>) {
    if let (Some(adb), Some(serial)) = (adb, serial) {
        let _ = adb::remove_reverse(adb, serial, protocol::PORT).await;
    }
}

async fn handle_client(
    stream: TcpStream,
    display: DisplayInfo,
    ffmpeg: std::path::PathBuf,
    req: SessionRequest,
    status: Arc<Mutex<SessionStatus>>,
    stop: Arc<AtomicBool>,
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

    let codec = pick_codec(&hello, req.prefer_hevc);
    let (dec_w, dec_h, dec_fps, hw) = codec_limit(&hello, &codec);
    let (mut width, mut height) =
        if req.match_device && hello.screen_width > 0 && hello.screen_height > 0 {
            fit_to_device(
                display.width,
                display.height,
                hello.screen_width,
                hello.screen_height,
                req.scale,
                dec_w,
                dec_h,
            )
        } else {
            let (w, h) =
                fit_resolution(display.width, display.height, req.max_width, req.max_height);
            if dec_w > 0 && dec_h > 0 {
                fit_resolution(w, h, dec_w, dec_h)
            } else {
                (w, h)
            }
        };
    let align = hello.alignment.max(2);
    width = (width / align * align).max(align);
    height = (height / align * align).max(align);

    let fps = adapted_fps(req.fps, hello.max_fps, dec_fps, hw);
    let auto_br = auto_bitrate(width, height, fps);
    let bitrate_kbps = if hw {
        auto_br.min(req.bitrate_kbps).max(4_000)
    } else {
        auto_br.min(req.bitrate_kbps).min(12_000).max(4_000)
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
    };
    let payload = serde_json::to_vec(&cfg)?;
    protocol::write_message(&mut writer, protocol::MSG_CONFIG, 0, &payload).await?;

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

    let mut session = None;
    let mut last_err = None;
    for enc in encoder::encoder_fallback_chain(&codec) {
        match encoder::start_encoder(&ffmpeg, &display, &settings, enc) {
            Ok(s) => {
                tracing::info!("using encoder {enc}");
                session = Some(s);
                break;
            }
            Err(err) => {
                tracing::warn!("encoder {enc} failed: {err:#}");
                last_err = Some(err);
            }
        }
    }
    if session.is_none() {
        match encoder::start_encoder_gdigrab(
            &ffmpeg,
            &display,
            &settings,
            encoder::encoder_fallback_chain(&codec)[0],
        ) {
            Ok(s) => session = Some(s),
            Err(err) => anyhow::bail!("启动编码器失败: {err:#}; 先前: {last_err:?}"),
        }
    }
    let mut session = session.context("no encoder")?;
    let hevc = codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265");
    let t0 = std::time::Instant::now();
    let audio_stop = Arc::new(AtomicBool::new(false));
    let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel::<crate::audio::AudioPacket>(48);
    if audio_enabled {
        match crate::audio::start_loopback(audio_tx, audio_stop.clone(), t0) {
            Ok(()) => tracing::info!("audio loopback started"),
            Err(err) => {
                tracing::warn!("audio loopback unavailable: {err:#}");
            }
        }
    }

    let display_for_input = display.clone();
    let stop_read = stop.clone();
    let reader_task = tokio::spawn(async move {
        loop {
            if stop_read.load(Ordering::Relaxed) {
                break;
            }
            match protocol::read_message(&mut reader).await {
                Ok(msg) if msg.ty == protocol::MSG_TOUCH => {
                    if let Ok(ev) = protocol::TouchEvent::parse(&msg.payload) {
                        input::inject_touch(&display_for_input, ev);
                    }
                }
                Ok(msg) if msg.ty == protocol::MSG_HEARTBEAT => {}
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });

    while !stop.load(Ordering::Relaxed) {
        while let Ok(pkt) = audio_rx.try_recv() {
            let payload = protocol::with_pts(pkt.pts_us, &pkt.pcm);
            if let Err(err) =
                protocol::write_message(&mut writer, protocol::MSG_AUDIO, 0, &payload).await
            {
                tracing::warn!("send audio failed: {err:#}");
                break;
            }
        }
        match session.rx.recv_timeout(Duration::from_millis(2)) {
            Ok(pkt) => {
                if let Err(err) = write_video_packet(&mut writer, t0, &pkt).await {
                    tracing::warn!("send video failed: {err:#}");
                    break;
                }
                while let Ok(ap) = audio_rx.try_recv() {
                    let audio_payload = protocol::with_pts(ap.pts_us, &ap.pcm);
                    if protocol::write_message(&mut writer, protocol::MSG_AUDIO, 0, &audio_payload)
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                if let Ok(mut s) = status.lock() {
                    s.frames += 1;
                    if s.frames % 60 == 0 {
                        s.detail = format!("已发送 {} 帧", s.frames);
                    }
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
                        if let Err(err) = write_bootstrap(&mut writer, t0, &bootstrap).await {
                            tracing::warn!("send bootstrap after encoder restart failed: {err:#}");
                            break;
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

async fn write_video_packet<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    t0: std::time::Instant,
    pkt: &EncodedPacket,
) -> Result<()> {
    let payload = protocol::with_pts(t0.elapsed().as_micros() as u64, &pkt.data);
    protocol::write_message(writer, protocol::MSG_VIDEO, video_flags(pkt), &payload).await
}

async fn write_bootstrap<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    t0: std::time::Instant,
    packets: &[EncodedPacket],
) -> Result<()> {
    for pkt in packets {
        write_video_packet(writer, t0, pkt).await?;
    }
    Ok(())
}

fn restart_encoder_with_bootstrap(
    ffmpeg: &std::path::PathBuf,
    display: &DisplayInfo,
    settings: &EncodeSettings,
    hevc: bool,
) -> Result<(encoder::EncoderSession, Vec<EncodedPacket>)> {
    let session = encoder::start_encoder_gdigrab(ffmpeg, display, settings, &settings.encoder)?;
    let bootstrap = annexb::recv_bootstrap(&session.rx, Duration::from_secs(3), hevc)?;
    Ok((session, bootstrap))
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
        hello
            .hevc_limit
            .as_ref()
            .map(|l| {
                (l.width as u64)
                    .saturating_mul(l.height as u64)
                    .saturating_mul(l.fps.max(24) as u64)
            })
            .unwrap_or(0)
    } else {
        0
    };
    // Treble/GSI HEVC is often broken or much slower; stay on AVC unless HEVC is clearly larger.
    if hello.gsi && avc_ok && hevc_score < avc_score.saturating_mul(2) {
        return "avc".into();
    }
    if prefer_hevc && hevc_ok && hevc_score >= avc_score {
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
    let (mut box_w, mut box_h) = if src_w >= src_h {
        if dev_w >= dev_h {
            (dev_w, dev_h)
        } else {
            (dev_h, dev_w)
        }
    } else if dev_h >= dev_w {
        (dev_w, dev_h)
    } else {
        (dev_h, dev_w)
    };
    if dec_w > 0 && dec_h > 0 {
        let (lim_w, lim_h) = if box_w >= box_h {
            if dec_w >= dec_h {
                (dec_w, dec_h)
            } else {
                (dec_h, dec_w)
            }
        } else if dec_h >= dec_w {
            (dec_w, dec_h)
        } else {
            (dec_h, dec_w)
        };
        box_w = box_w.min(lim_w);
        box_h = box_h.min(lim_h);
    }
    let scale = (scale as f64).clamp(0.35, 1.0);
    let cap_w = ((box_w as f64 * scale) as u32).max(16);
    let cap_h = ((box_h as f64 * scale) as u32).max(16);
    fit_resolution(src_w, src_h, cap_w, cap_h)
}

fn auto_bitrate(width: u32, height: u32, fps: u32) -> u32 {
    let mp = (width as f64 * height as f64) / 1_000_000.0;
    let kbps = (mp * fps as f64 * 120.0) as u32;
    kbps.clamp(6_000, 40_000)
}

fn fit_resolution(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let mut w = src_w.max(2);
    let mut h = src_h.max(2);
    if w > max_w || h > max_h {
        let scale = (max_w as f64 / w as f64).min(max_h as f64 / h as f64);
        w = ((w as f64 * scale) as u32) & !1;
        h = ((h as f64 * scale) as u32) & !1;
    }
    (w.max(2), h.max(2))
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
        assert_eq!(pick_codec(&h, false), "avc");
    }

    #[test]
    fn software_decode_caps_fps() {
        assert_eq!(adapted_fps(60, 60, 60, false), 45);
        assert_eq!(adapted_fps(120, 60, 60, true), 60);
        assert_eq!(adapted_fps(60, 60, 30, true), 30);
    }

    #[test]
    fn avc_level_matches_common_phones() {
        assert_eq!(crate::encoder::avc_level(1920, 1080, 60), "4.1");
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
