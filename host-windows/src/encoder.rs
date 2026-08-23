use anyhow::{Context, Result};
use std::io::{BufReader, Read};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::displays::DisplayInfo;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone)]
pub struct EncodeSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub codec: String, // "avc" | "hevc"
    pub encoder: String,
    pub profile: String, // "main" | "baseline"
}

#[derive(Debug, Clone)]
pub struct EncodedPacket {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub codec_config: bool,
}

pub struct EncoderSession {
    child: Child,
    pub rx: Receiver<EncodedPacket>,
}

impl EncoderSession {
    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for EncoderSession {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn find_ffmpeg() -> Result<PathBuf> {
    which::which("ffmpeg").context("找不到 ffmpeg，请安装并加入 PATH")
}

pub fn pick_encoder(codec: &str) -> &'static str {
    // FFmpeg on this machine ships nvenc/qsv/amf; runtime probe happens at spawn.
    if codec == "hevc" {
        "hevc_nvenc"
    } else {
        "h264_nvenc"
    }
}

pub fn encoder_fallback_chain(codec: &str) -> Vec<&'static str> {
    if codec == "hevc" {
        vec!["hevc_nvenc", "hevc_qsv", "hevc_amf", "libx265"]
    } else {
        vec!["h264_nvenc", "h264_qsv", "h264_amf", "libx264"]
    }
}

pub fn start_encoder(
    ffmpeg: &PathBuf,
    display: &DisplayInfo,
    settings: &EncodeSettings,
    encoder: &str,
) -> Result<EncoderSession> {
    let args = build_args(display, settings, encoder);
    tracing::info!("ffmpeg {}", args.join(" "));

    let mut cmd = Command::new(ffmpeg);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd.spawn().context("spawn ffmpeg")?;
    let stdout = child.stdout.take().context("ffmpeg stdout")?;
    let stderr = child.stderr.take().context("ffmpeg stderr")?;

    thread::spawn(move || {
        let mut r = BufReader::new(stderr);
        let mut buf = String::new();
        if r.read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
            for line in buf.lines() {
                tracing::debug!("ffmpeg: {line}");
            }
            tracing::info!("ffmpeg stderr (last):\n{}", tail(&buf, 12));
        }
    });

    let (tx, rx) = mpsc::sync_channel(24);
    thread::spawn(move || {
        if let Err(err) = pump_annexb(stdout, tx) {
            tracing::warn!("encoder pump ended: {err:#}");
        }
    });

    Ok(EncoderSession { child, rx })
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines.iter().rev().take(n).rev().cloned().collect::<Vec<_>>().join("\n")
}

fn build_args(display: &DisplayInfo, settings: &EncodeSettings, encoder: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "warning".into(),
        "-fflags".into(),
        "nobuffer".into(),
        "-flags".into(),
        "low_delay".into(),
        "-probesize".into(),
        "32".into(),
        "-analyzeduration".into(),
        "0".into(),
    ];

    // Desktop Duplication (low latency). output_idx matches DXGI order.
    let filter = format!(
        "ddagrab=output_idx={}:framerate={}:draw_mouse=1,hwdownload,format=bgra,format=yuv420p,scale={}:{}:flags=fast_bilinear",
        display.dxgi_index, settings.fps, settings.width, settings.height
    );
    args.extend([
        "-filter_complex".into(),
        filter,
        "-an".into(),
        "-c:v".into(),
        encoder.to_string(),
    ]);
    args.extend(encoder_flags(encoder, settings));
    args.extend(output_mux_args(encoder));
    args
}

fn output_mux_args(encoder: &str) -> Vec<String> {
    let muxer = if encoder.contains("hevc") || encoder.contains("x265") {
        "hevc"
    } else {
        "h264"
    };
    vec![
        "-bsf:v".into(),
        "dump_extra".into(),
        "-f".into(),
        muxer.into(),
        "-flush_packets".into(),
        "1".into(),
        "pipe:1".into(),
    ]
}

/// gdigrab fallback when ddagrab fails at runtime — caller swaps by rebuilding args.
pub fn start_encoder_gdigrab(
    ffmpeg: &PathBuf,
    display: &DisplayInfo,
    settings: &EncodeSettings,
    encoder: &str,
) -> Result<EncoderSession> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "warning".into(),
        "-fflags".into(),
        "nobuffer".into(),
        "-flags".into(),
        "low_delay".into(),
        "-f".into(),
        "gdigrab".into(),
        "-framerate".into(),
        settings.fps.to_string(),
        "-offset_x".into(),
        display.x.to_string(),
        "-offset_y".into(),
        display.y.to_string(),
        "-video_size".into(),
        format!("{}x{}", display.width, display.height),
        "-draw_mouse".into(),
        "1".into(),
        "-i".into(),
        "desktop".into(),
        "-an".into(),
        "-vf".into(),
        format!("format=yuv420p,scale={}:{}:flags=fast_bilinear", settings.width, settings.height),
        "-c:v".into(),
        encoder.to_string(),
    ];
    args.extend(encoder_flags(encoder, settings));
    args.extend(output_mux_args(encoder));

    tracing::info!("ffmpeg(gdigrab) {}", args.join(" "));
    let mut cmd = Command::new(ffmpeg);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
    let mut child = cmd.spawn().context("spawn ffmpeg gdigrab")?;
    let stdout = child.stdout.take().context("ffmpeg stdout")?;
    let stderr = child.stderr.take().context("ffmpeg stderr")?;
    thread::spawn(move || {
        let mut r = BufReader::new(stderr);
        let mut buf = String::new();
        if r.read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
            tracing::info!("ffmpeg stderr:\n{}", tail(&buf, 16));
        }
    });
    let (tx, rx) = mpsc::sync_channel(24);
    thread::spawn(move || {
        let _ = pump_annexb(stdout, tx);
    });
    Ok(EncoderSession { child, rx })
}

fn encoder_flags(encoder: &str, settings: &EncodeSettings) -> Vec<String> {
    let br = format!("{}k", settings.bitrate_kbps);
    let buf = format!("{}k", (settings.bitrate_kbps / 2).max(4000));
    let gop = (settings.fps / 2).max(15).to_string();
    let level = avc_level(settings.width, settings.height, settings.fps).to_string();
    if encoder.contains("nvenc") {
        vec![
            "-preset".into(), "p1".into(),
            "-tune".into(), "ll".into(),
            "-rc".into(), "cbr".into(),
            "-b:v".into(), br.clone(),
            "-maxrate".into(), format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(), buf,
            "-bf".into(), "0".into(),
            "-g".into(), gop,
            "-slices".into(), "1".into(),
            "-profile:v".into(), settings.profile.clone(),
            "-level:v".into(), level,
            "-forced-idr".into(), "1".into(),
            "-aud".into(), "1".into(),
            "-delay".into(), "0".into(),
            "-rc-lookahead".into(), "0".into(),
            "-zerolatency".into(), "1".into(),
        ]
    } else if encoder.contains("qsv") {
        vec![
            "-preset".into(), "veryfast".into(),
            "-b:v".into(), br,
            "-maxrate".into(), format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(), buf,
            "-bf".into(), "0".into(),
            "-g".into(), gop,
            "-profile:v".into(), settings.profile.clone(),
            "-look_ahead".into(), "0".into(),
        ]
    } else if encoder.contains("amf") {
        vec![
            "-quality".into(), "speed".into(),
            "-rc".into(), "cbr".into(),
            "-b:v".into(), br,
            "-bf".into(), "0".into(),
            "-g".into(), gop,
            "-profile:v".into(), settings.profile.clone(),
            "-usage".into(), "ultralowlatency".into(),
        ]
    } else if encoder.contains("x265") || encoder.contains("hevc") {
        vec![
            "-preset".into(), "ultrafast".into(),
            "-tune".into(), "zerolatency".into(),
            "-b:v".into(), br,
            "-maxrate".into(), format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(), buf,
            "-bf".into(), "0".into(),
            "-g".into(), gop,
            "-pix_fmt".into(), "yuv420p".into(),
        ]
    } else {
        vec![
            "-preset".into(), "ultrafast".into(),
            "-tune".into(), "zerolatency".into(),
            "-b:v".into(), br,
            "-maxrate".into(), format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(), buf,
            "-bf".into(), "0".into(),
            "-g".into(), gop,
            "-pix_fmt".into(), "yuv420p".into(),
            "-profile:v".into(), settings.profile.clone(),
            "-level:v".into(), level,
            "-x264-params".into(), format!("repeat-headers=1:scenecut=0:sliced-threads=0:level={}", avc_level(settings.width, settings.height, settings.fps)).into(),
        ]
    }
}

pub fn avc_level(width: u32, height: u32, fps: u32) -> &'static str {
    let area = width.saturating_mul(height);
    if area <= 1280 * 720 && fps <= 30 {
        "3.1"
    } else if area <= 1280 * 720 {
        "3.2"
    } else if area <= 1920 * 1088 && fps <= 30 {
        "4.0"
    } else if area <= 1920 * 1088 {
        "4.1"
    } else if area <= 2560 * 1440 {
        "5.0"
    } else {
        "5.1"
    }
}

fn pump_annexb(mut stdout: impl Read, tx: mpsc::SyncSender<EncodedPacket>) -> Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut acc = Vec::with_capacity(256 * 1024);
    let mut au = Vec::new();
    let mut au_has_vcl = false;
    let mut au_key = false;
    let mut sps_pps = Vec::new();
    let mut sent_cfg = false;
    let mut drop_until_key = false;

    loop {
        let n = stdout.read(&mut buf)?;
        if n == 0 {
            flush_au(
                &mut au,
                &mut au_has_vcl,
                &mut au_key,
                &sps_pps,
                &tx,
                &mut drop_until_key,
            );
            break;
        }
        acc.extend_from_slice(&buf[..n]);
        let nals = split_annexb(&mut acc);
        for nal in nals {
            let ty = h264_nal_type(&nal);
            if ty == 7 || ty == 8 {
                if ty == 7 {
                    sps_pps.clear();
                }
                sps_pps.extend_from_slice(&nal);
                continue;
            }
            if !sent_cfg && !sps_pps.is_empty() {
                let _ = tx.send(EncodedPacket {
                    data: sps_pps.clone(),
                    keyframe: false,
                    codec_config: true,
                });
                sent_cfg = true;
            }
            let is_vcl = matches!(ty, 1 | 5);
            if ty == 9 || (is_vcl && au_has_vcl) {
                flush_au(
                    &mut au,
                    &mut au_has_vcl,
                    &mut au_key,
                    &sps_pps,
                    &tx,
                    &mut drop_until_key,
                );
            }
            if ty == 5 {
                au_key = true;
            }
            if is_vcl {
                au_has_vcl = true;
            }
            au.extend_from_slice(&nal);
        }
    }
    Ok(())
}

fn flush_au(
    au: &mut Vec<u8>,
    has_vcl: &mut bool,
    key: &mut bool,
    sps_pps: &[u8],
    tx: &mpsc::SyncSender<EncodedPacket>,
    drop_until_key: &mut bool,
) {
    if au.is_empty() {
        return;
    }
    let is_key = *key;
    if *drop_until_key && !is_key {
        au.clear();
        *has_vcl = false;
        *key = false;
        return;
    }
    let mut data = Vec::with_capacity(sps_pps.len() + au.len());
    if is_key && !sps_pps.is_empty() {
        data.extend_from_slice(sps_pps);
    }
    data.append(au);
    let pkt = EncodedPacket {
        data,
        keyframe: is_key,
        codec_config: false,
    };
    match tx.try_send(pkt) {
        Ok(()) => {
            *drop_until_key = false;
        }
        Err(mpsc::TrySendError::Full(pkt)) => {
            if pkt.keyframe {
                let _ = tx.send(pkt);
                *drop_until_key = false;
            } else {
                *drop_until_key = true;
            }
        }
        Err(mpsc::TrySendError::Disconnected(_)) => {}
    }
    *has_vcl = false;
    *key = false;
}

fn h264_nal_type(nal: &[u8]) -> u8 {
    // nal: start code + header
    let i = if nal.len() >= 4 && nal[0] == 0 && nal[1] == 0 && nal[2] == 0 && nal[3] == 1 {
        4
    } else if nal.len() >= 3 && nal[0] == 0 && nal[1] == 0 && nal[2] == 1 {
        3
    } else {
        return 0;
    };
    nal.get(i).map(|b| b & 0x1F).unwrap_or(0)
}

/// Split complete NALs from `acc`, leaving a trailing incomplete fragment.
fn split_annexb(acc: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 < acc.len() {
        if acc[i] == 0 && acc[i + 1] == 0 {
            if acc[i + 2] == 1 {
                starts.push(i);
                i += 3;
                continue;
            }
            if i + 3 < acc.len() && acc[i + 2] == 0 && acc[i + 3] == 1 {
                starts.push(i);
                i += 4;
                continue;
            }
        }
        i += 1;
    }
    if starts.len() < 2 {
        return Vec::new();
    }
    let mut nals = Vec::new();
    for w in starts.windows(2) {
        nals.push(acc[w[0]..w[1]].to_vec());
    }
    let last = *starts.last().unwrap();
    acc.drain(..last);
    nals
}
