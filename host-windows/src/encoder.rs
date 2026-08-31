use anyhow::{Context, Result};
use std::io::BufReader;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::displays::DisplayInfo;
use lighting_host::annexb;

pub use lighting_host::annexb::EncodedPacket;

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

pub struct EncoderSession {
    child: Option<Child>,
    pub rx: Receiver<EncodedPacket>,
}

impl EncoderSession {
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Kill ffmpeg without blocking the accept loop on `wait()`.
    pub fn stop_in_background(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = std::thread::Builder::new()
                .name("lighting-ffmpeg-wait".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}

impl Drop for EncoderSession {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn find_ffmpeg() -> Result<PathBuf> {
    if let Ok(p) = which::which("ffmpeg") {
        return Ok(p);
    }
    let mut candidates = Vec::new();
    if let Ok(runtime) = std::env::var("LIGHTING_RUNTIME_DIR") {
        candidates.push(PathBuf::from(&runtime).join("ffmpeg").join("bin").join("ffmpeg.exe"));
        candidates.push(PathBuf::from(&runtime).join("ffmpeg.exe"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ffmpeg.exe"));
            candidates.push(dir.join("ffmpeg").join("bin").join("ffmpeg.exe"));
        }
    }
    if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
        return Ok(found);
    }
    anyhow::bail!("找不到 ffmpeg。便携版首次启动会自动下载；也可手动安装并加入 PATH")
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
    capture_filter: &str,
) -> Result<EncoderSession> {
    let args = build_args(settings, encoder, capture_filter);
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

    // Keep at most ~2 encoded AUs queued; annexb drops P-frames when full
    // so USB backpressure cannot turn into hundreds of ms of glass latency.
    let (tx, rx) = mpsc::sync_channel(2);
    let hevc = is_hevc(&settings.codec);
    thread::spawn(move || {
        if let Err(err) = annexb::pump_annexb(stdout, tx, hevc) {
            tracing::warn!("encoder pump ended: {err:#}");
        }
    });

    Ok(EncoderSession {
        child: Some(child),
        rx,
    })
}

fn is_hevc(codec: &str) -> bool {
    codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265")
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines
        .iter()
        .rev()
        .take(n)
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_args(settings: &EncodeSettings, encoder: &str, capture_filter: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "warning".into(),
        "-fflags".into(),
        "nobuffer+flush_packets".into(),
        "-flags".into(),
        "low_delay".into(),
        "-probesize".into(),
        "32".into(),
        "-analyzeduration".into(),
        "0".into(),
        "-thread_queue_size".into(),
        "2".into(),
    ];

    args.extend([
        "-filter_complex".into(),
        capture_filter.to_string(),
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
        "nobuffer+flush_packets".into(),
        "-flags".into(),
        "low_delay".into(),
        "-probesize".into(),
        "32".into(),
        "-analyzeduration".into(),
        "0".into(),
        "-thread_queue_size".into(),
        "2".into(),
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
        lighting_host::capture_graph::gdigrab_vf(
            display.width,
            display.height,
            settings.width,
            settings.height,
        ),
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
    let (tx, rx) = mpsc::sync_channel(2);
    let hevc = is_hevc(&settings.codec);
    thread::spawn(move || {
        if let Err(err) = annexb::pump_annexb(stdout, tx, hevc) {
            tracing::warn!("encoder pump ended: {err:#}");
        }
    });
    Ok(EncoderSession {
        child: Some(child),
        rx,
    })
}

/// ~2-frame VBV so rate control does not hold frames for half a second.
fn vbv_bufsize_kb(bitrate_kbps: u32, fps: u32) -> u32 {
    lighting_host::session_policy::vbv_bufsize_kb(bitrate_kbps, fps)
}

fn encoder_flags(encoder: &str, settings: &EncodeSettings) -> Vec<String> {
    let br = format!("{}k", settings.bitrate_kbps);
    let buf = format!("{}k", vbv_bufsize_kb(settings.bitrate_kbps, settings.fps));
    // Keyframe every ~1s: short enough to recover after drops, long enough
    // that IDR spikes do not dominate USB3 bandwidth.
    let gop = settings.fps.max(30).to_string();
    let level = avc_level(settings.width, settings.height, settings.fps).to_string();
    if encoder.contains("nvenc") {
        vec![
            "-preset".into(),
            "p1".into(),
            "-tune".into(),
            "ll".into(),
            "-rc".into(),
            "cbr".into(),
            "-b:v".into(),
            br.clone(),
            "-maxrate".into(),
            format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(),
            buf,
            "-bf".into(),
            "0".into(),
            "-g".into(),
            gop,
            "-slices".into(),
            "1".into(),
            "-profile:v".into(),
            settings.profile.clone(),
            "-forced-idr".into(),
            "1".into(),
            "-aud".into(),
            "1".into(),
            "-delay".into(),
            "0".into(),
            "-rc-lookahead".into(),
            "0".into(),
            "-zerolatency".into(),
            "1".into(),
            // Spatial AQ improves detail at the same bitrate with negligible latency cost.
            "-spatial-aq".into(),
            "1".into(),
            "-temporal-aq".into(),
            "0".into(),
        ]
    } else if encoder.contains("qsv") {
        vec![
            "-preset".into(),
            "veryfast".into(),
            "-b:v".into(),
            br,
            "-maxrate".into(),
            format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(),
            buf,
            "-bf".into(),
            "0".into(),
            "-g".into(),
            gop,
            "-profile:v".into(),
            settings.profile.clone(),
            "-look_ahead".into(),
            "0".into(),
            "-async_depth".into(),
            "1".into(),
        ]
    } else if encoder.contains("amf") {
        vec![
            "-quality".into(),
            "speed".into(),
            "-rc".into(),
            "cbr".into(),
            "-b:v".into(),
            br,
            "-maxrate".into(),
            format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(),
            buf,
            "-bf".into(),
            "0".into(),
            "-g".into(),
            gop,
            "-profile:v".into(),
            settings.profile.clone(),
            "-usage".into(),
            "ultralowlatency".into(),
        ]
    } else if encoder.contains("x265") || encoder.contains("hevc") {
        vec![
            "-preset".into(),
            "ultrafast".into(),
            "-tune".into(),
            "zerolatency".into(),
            "-b:v".into(),
            br,
            "-maxrate".into(),
            format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(),
            buf,
            "-bf".into(),
            "0".into(),
            "-g".into(),
            gop,
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]
    } else {
        vec![
            "-preset".into(),
            "ultrafast".into(),
            "-tune".into(),
            "zerolatency".into(),
            "-b:v".into(),
            br,
            "-maxrate".into(),
            format!("{}k", settings.bitrate_kbps),
            "-bufsize".into(),
            buf,
            "-bf".into(),
            "0".into(),
            "-g".into(),
            gop,
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-profile:v".into(),
            settings.profile.clone(),
            "-level:v".into(),
            level,
            "-x264-params".into(),
            format!(
                "repeat-headers=1:scenecut=0:sliced-threads=0:sync-lookahead=0:rc-lookahead=0:level={}",
                avc_level(settings.width, settings.height, settings.fps)
            )
            .into(),
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
        // 1080p60 exceeds Level 4.1 MaxMBPS; NVENC rejects 4.1 as Invalid Level.
        "4.2"
    } else if area <= 2560 * 1440 {
        "5.0"
    } else {
        "5.1"
    }
}
