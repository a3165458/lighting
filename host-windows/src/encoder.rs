use anyhow::{Context, Result};
use std::io::BufReader;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::displays::DisplayInfo;
use lighting_host::annexb;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use windows::Win32::Foundation::{
    CloseHandle, FALSE, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT, SetHandleInformation,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::Threading::{
    GetCurrentThread, OpenThread, SetPriorityClass, SetProcessInformation, SetThreadPriority,
    HIGH_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
    PROCESS_POWER_THROTTLING_STATE, ProcessPowerThrottling, THREAD_PRIORITY_HIGHEST,
    THREAD_QUERY_INFORMATION, THREAD_SET_INFORMATION,
};

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
    /// False when the tablet paints a local OS pointer overlay.
    pub draw_mouse: bool,
    /// NVENC `-surfaces`. 1 is Sunshine ULL; 2 if ffmpeg never emits IDR.
    pub nvenc_surfaces: u32,
    /// NVENC `-rc`. `cbr_ld_hq` first; `cbr` if that ffmpeg rejects it.
    pub nvenc_rc: String,
    /// AMF `-rc`. `vbr_latency` first; `cbr` if that ffmpeg rejects it.
    pub amf_rc: String,
}

pub struct EncoderSession {
    child: Option<Child>,
    pub rx: tokio::sync::mpsc::Receiver<EncodedPacket>,
    boost_stop: Option<Arc<AtomicBool>>,
}

impl EncoderSession {
    pub fn stop(&mut self) {
        if let Some(flag) = self.boost_stop.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Kill ffmpeg without blocking the accept loop on `wait()`.
    pub fn stop_in_background(mut self) {
        if let Some(flag) = self.boost_stop.take() {
            flag.store(true, Ordering::Relaxed);
        }
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
    which::which("ffmpeg")
        .context("找不到 ffmpeg。便携版首次启动会自动下载；也可手动安装并加入 PATH")
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
    encoder_fallback_chain_for(codec, 0)
}

pub fn encoder_fallback_chain_for(codec: &str, vendor_id: u32) -> Vec<&'static str> {
    lighting_host::session_policy::encoder_fallback_chain(codec, vendor_id)
}

pub fn start_encoder(
    ffmpeg: &PathBuf,
    display: &DisplayInfo,
    settings: &EncodeSettings,
    encoder: &str,
    capture_filter: &str,
) -> Result<EncoderSession> {
    let Some(capture) = display.dxgi else {
        return start_encoder_gdigrab(ffmpeg, display, settings, encoder);
    };
    let args = build_args(capture, settings, encoder, capture_filter);
    tracing::info!("ffmpeg {}", args.join(" "));

    let mut cmd = Command::new(ffmpeg);
    let (stdout, write) = ffmpeg_stdout_pipe()?;
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(write))
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | HIGH_PRIORITY_CLASS.0);

    let mut child = cmd.spawn().context("spawn ffmpeg")?;
    raise_process_priority(&child);
    let stderr = child.stderr.take().context("ffmpeg stderr")?;

    thread::spawn(move || {
        raise_thread_priority();
        let mut r = BufReader::new(stderr);
        let mut buf = String::new();
        if r.read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
            for line in buf.lines() {
                tracing::debug!("ffmpeg: {line}");
            }
            tracing::info!("ffmpeg stderr (last):\n{}", tail(&buf, 12));
        }
    });

    let hevc = is_hevc(&settings.codec);
    let rx = spawn_annexb_pump(stdout, hevc);
    let pid = child.id();
    Ok(EncoderSession {
        child: Some(child),
        rx,
        boost_stop: spawn_ffmpeg_thread_boost(pid),
    })
}

fn spawn_annexb_pump(
    stdout: impl std::io::Read + AsRawHandle + Send + 'static,
    hevc: bool,
) -> tokio::sync::mpsc::Receiver<EncodedPacket> {
    // One encoded AU: a deeper queue is glass latency, not a USB cushion.
    let cap = lighting_host::session_policy::encoded_queue_capacity().max(1);
    let (tx, rx) = tokio::sync::mpsc::channel(cap);
    thread::spawn(move || {
        raise_thread_priority();
        if let Err(err) = annexb::pump_annexb_with_available(stdout, tx, hevc, |s| pipe_bytes_available(s)) {
            tracing::warn!("encoder pump ended: {err:#}");
        }
    });
    rx
}

/// Windows default anonymous-pipe buffer is 4 KB. ffmpeg then emits a
/// 20–50 KB AU as many short writes; PeekNamedPipe goes quiet between them
/// and we flush a partial picture. Inherit a larger pipe as stdout instead.
fn ffmpeg_stdout_pipe() -> Result<(std::fs::File, std::fs::File)> {
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    let mut sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    };
    unsafe {
        CreatePipe(
            &mut read,
            &mut write,
            Some(&sa as *const SECURITY_ATTRIBUTES),
            lighting_host::session_policy::ffmpeg_pipe_buffer_bytes(),
        )
        .context("CreatePipe ffmpeg stdout")?;
        SetHandleInformation(read, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
            .context("ffmpeg stdout read handle must not be inherited")?;
        Ok((
            std::fs::File::from_raw_handle(read.0),
            std::fs::File::from_raw_handle(write.0),
        ))
    }
}

fn pipe_bytes_available(stdout: &impl AsRawHandle) -> Option<usize> {
    let mut n = 0u32;
    unsafe {
        PeekNamedPipe(
            HANDLE(stdout.as_raw_handle()),
            None,
            0,
            None,
            Some(&mut n),
            None,
        )
        .ok()?;
    }
    Some(n as usize)
}

fn raise_thread_priority() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    }
    lighting_host::annexb::enter_mmcss();
}

fn raise_process_priority(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    unsafe {
        let handle = HANDLE(child.as_raw_handle());
        let _ = SetPriorityClass(handle, HIGH_PRIORITY_CLASS);
        disable_power_throttling(handle);
        crate::displays::raise_gpu_scheduling(handle);
    }
}

/// Sunshine capture is CRITICAL in-process. ffmpeg's ddagrab/NVENC threads
/// are spawned at NORMAL inside HIGH_PRIORITY_CLASS; a game on MMCSS then
/// parks a ready DXGI frame on a 15.6 ms quanta. MMCSS is current-thread
/// only, so walk the child's threads and pin HIGHEST (max in HIGH class).
fn spawn_ffmpeg_thread_boost(pid: u32) -> Option<Arc<AtomicBool>> {
    if !lighting_host::session_policy::boost_ffmpeg_child_threads() {
        return None;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let _ = thread::Builder::new()
        .name("lighting-ffmpeg-boost".into())
        .spawn(move || {
            raise_thread_priority();
            let mut last = 0u32;
            while !stop.load(Ordering::Relaxed) {
                let n = raise_child_threads(pid);
                if n > 0 && n != last {
                    tracing::info!("ffmpeg threads HIGHEST n={n}");
                    last = n;
                }
                thread::sleep(Duration::from_millis(250));
            }
        });
    Some(flag)
}

fn raise_child_threads(pid: u32) -> u32 {
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) {
            Ok(h) => h,
            Err(_) => return 0,
        };
        let mut te = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut n = 0u32;
        if Thread32First(snap, &mut te).is_ok() {
            loop {
                if te.th32OwnerProcessID == pid {
                    if let Ok(th) = OpenThread(
                        THREAD_SET_INFORMATION | THREAD_QUERY_INFORMATION,
                        FALSE,
                        te.th32ThreadID,
                    ) {
                        let _ = SetThreadPriority(th, THREAD_PRIORITY_HIGHEST);
                        let _ = CloseHandle(th);
                        n += 1;
                    }
                }
                if Thread32Next(snap, &mut te).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        n
    }
}

/// Same EcoQoS / timer-resolution opt-out as the host process. ffmpeg is a
/// child: Windows 11 would otherwise park it on E-cores while the game has
/// focus on the virtual panel.
fn disable_power_throttling(handle: HANDLE) {
    unsafe {
        let mut state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            StateMask: 0,
        };
        if SetProcessInformation(
            handle,
            ProcessPowerThrottling,
            &state as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&state) as u32,
        )
        .is_err()
        {
            state.ControlMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
            let _ = SetProcessInformation(
                handle,
                ProcessPowerThrottling,
                &state as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&state) as u32,
            );
        }
    }
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

fn build_args(
    capture: lighting_host::capture_graph::DxgiCapture,
    settings: &EncodeSettings,
    encoder: &str,
    capture_filter: &str,
) -> Vec<String> {
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
        lighting_host::session_policy::capture_thread_queue_size().to_string(),
        "-filter_threads".into(),
        lighting_host::session_policy::ffmpeg_filter_threads().to_string(),
        "-filter_complex_threads".into(),
        lighting_host::session_policy::ffmpeg_filter_threads().to_string(),
        "-avioflags".into(),
        "direct".into(),
        "-auto_conversion_filters".into(),
        if lighting_host::session_policy::ffmpeg_auto_conversion_filters() {
            "1".into()
        } else {
            "0".into()
        },
    ];

    args.extend(capture.device_args());
    args.extend(lighting_host::capture_graph::extra_hw_device_args(
        capture,
        capture_filter,
    ));
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
        "-muxdelay".into(),
        "0".into(),
        "-muxpreload".into(),
        "0".into(),
        "-max_delay".into(),
        "0".into(),
        "-max_interleave_delta".into(),
        lighting_host::session_policy::ffmpeg_max_interleave_delta().to_string(),
        "-fps_mode".into(),
        "passthrough".into(),
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
    anyhow::ensure!(
        display.width > 0 && display.height > 0,
        "所选显示器没有可捕获区域"
    );
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
        lighting_host::session_policy::capture_thread_queue_size().to_string(),
        "-filter_threads".into(),
        lighting_host::session_policy::ffmpeg_filter_threads().to_string(),
        "-filter_complex_threads".into(),
        lighting_host::session_policy::ffmpeg_filter_threads().to_string(),
        "-avioflags".into(),
        "direct".into(),
        "-auto_conversion_filters".into(),
        if lighting_host::session_policy::ffmpeg_auto_conversion_filters() {
            "1".into()
        } else {
            "0".into()
        },
    ];
    args.extend(lighting_host::capture_graph::gdigrab_input_args(
        display.x,
        display.y,
        display.width,
        display.height,
        settings.fps,
        settings.draw_mouse,
    ));
    args.extend([
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
    ]);
    args.extend(encoder_flags(encoder, settings));
    args.extend(output_mux_args(encoder));

    tracing::info!("ffmpeg(gdigrab) {}", args.join(" "));
    let mut cmd = Command::new(ffmpeg);
    let (stdout, write) = ffmpeg_stdout_pipe()?;
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(write))
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW | HIGH_PRIORITY_CLASS.0);
    let mut child = cmd.spawn().context("spawn ffmpeg gdigrab")?;
    raise_process_priority(&child);
    let stderr = child.stderr.take().context("ffmpeg stderr")?;
    thread::spawn(move || {
        raise_thread_priority();
        let mut r = BufReader::new(stderr);
        let mut buf = String::new();
        if r.read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
            tracing::info!("ffmpeg stderr:\n{}", tail(&buf, 16));
        }
    });
    let hevc = is_hevc(&settings.codec);
    let rx = spawn_annexb_pump(stdout, hevc);
    let pid = child.id();
    Ok(EncoderSession {
        child: Some(child),
        rx,
        boost_stop: spawn_ffmpeg_thread_boost(pid),
    })
}

/// One-frame VBV (Sunshine ULL). A 400 kb floor was ~2 frames at 120 Hz.
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
    // Do not pass muxer `-r`. Current ffmpeg FATAL-rejects it with
    // `-fps_mode passthrough`. Older builds then VSYNC_CFR and hold
    // every picture until the next 1/fps tick — one refresh vs the
    // laptop. SPS rewrite drops ddagrab's 8000 Hz VUI; Android sets
    // KEY_FRAME_RATE 120. GOP/VBV already use settings.fps.
    let mut flags = vec![
        // After -c:v: h264_amf/hevc_amf default flags=+loop clears the
        // global -flags low_delay. amfenc then uses output_delay =
        // max_b_frames + 1 (one extra encoded picture vs the laptop).
        // Sunshine sets AV_CODEC_FLAG_LOW_DELAY on the encoder context.
        "-flags:v".into(),
        lighting_host::session_policy::encoder_video_flags().into(),
    ];
    flags.extend(if encoder.contains("nvenc") {
        vec![
            "-preset".into(),
            "p1".into(),
            "-tune".into(),
            "ull".into(),
            "-surfaces".into(),
            settings.nvenc_surfaces.max(1).to_string(),
            "-multipass".into(),
            "disabled".into(),
            "-rc".into(),
            if settings.nvenc_rc.is_empty() {
                lighting_host::session_policy::nvenc_rc().into()
            } else {
                settings.nvenc_rc.clone()
            },
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
            "-refs".into(),
            lighting_host::session_policy::encoder_refs().to_string(),
            "-dpb_size".into(),
            lighting_host::session_policy::nvenc_dpb_size().to_string(),
            "-extra_sei".into(),
            if lighting_host::session_policy::nvenc_extra_sei() {
                "1".into()
            } else {
                "0".into()
            },
            "-a53cc".into(),
            if lighting_host::session_policy::nvenc_a53cc() {
                "1".into()
            } else {
                "0".into()
            },
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
            "-ldkfs".into(),
            lighting_host::session_policy::nvenc_ldkfs().to_string(),
            "-b_ref_mode".into(),
            "0".into(),
            // Sunshine disables Spatial AQ for ULL: CUDA complexity scan can
            // add a frame of host encode time.
            "-spatial-aq".into(),
            if lighting_host::session_policy::nvenc_spatial_aq() {
                "1".into()
            } else {
                "0".into()
            },
            "-temporal-aq".into(),
            "0".into(),
        ]
    } else if encoder.contains("qsv") {
        let mut qsv = vec![
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
            "-refs".into(),
            lighting_host::session_policy::encoder_refs().to_string(),
            "-forced_idr".into(),
            if lighting_host::session_policy::qsv_forced_idr() {
                "1".into()
            } else {
                "0".into()
            },
            "-profile:v".into(),
            settings.profile.clone(),
            "-async_depth".into(),
            "1".into(),
            "-low_power".into(),
            if lighting_host::session_policy::qsv_low_power() {
                "1".into()
            } else {
                "0".into()
            },
            "-extbrc".into(),
            "0".into(),
            "-adaptive_b".into(),
            "0".into(),
            "-scenario".into(),
            lighting_host::session_policy::qsv_scenario().into(),
            "-aud".into(),
            if lighting_host::session_policy::qsv_aud() {
                "1".into()
            } else {
                "0".into()
            },
            "-recovery_point_sei".into(),
            if lighting_host::session_policy::qsv_recovery_point_sei() {
                "1".into()
            } else {
                "0".into()
            },
            // NVENC already passes -slices 1. QSV NumSlice=0 lets MSDK
            // emit several VCL NALs per picture; annexb flushes on the
            // second VCL and the tablet paints a torn AU (one refresh).
            "-slices".into(),
            lighting_host::session_policy::encoder_slices().to_string(),
        ];
        // hevc_qsv rejects H.264-only private options and ffmpeg then
        // falls through to libx265 at 45 fps. HEVC DPB is rewritten in
        // annexb instead.
        if lighting_host::session_policy::qsv_h264_private_options(encoder) {
            qsv.extend([
                "-max_dec_frame_buffering".into(),
                lighting_host::session_policy::qsv_max_dec_frame_buffering().to_string(),
                "-a53cc".into(),
                if lighting_host::session_policy::qsv_a53cc() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-look_ahead".into(),
                "0".into(),
                "-vcm".into(),
                if lighting_host::session_policy::qsv_vcm() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-low_delay_brc".into(),
                "1".into(),
                "-mbbrc".into(),
                "0".into(),
                "-rdo".into(),
                if lighting_host::session_policy::qsv_rdo() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-adaptive_i".into(),
                if lighting_host::session_policy::qsv_adaptive_i() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-p_strategy".into(),
                lighting_host::session_policy::qsv_p_strategy().to_string(),
                "-pic_timing_sei".into(),
                if lighting_host::session_policy::qsv_pic_timing_sei() {
                    "1".into()
                } else {
                    "0".into()
                },
            ]);
        }
        if lighting_host::session_policy::qsv_hevc_private_options(encoder) {
            // ffmpeg qsvenc_hevc.c: these are HEVC-valid. Stripping them
            // with the H.264-only set left pic_timing_sei=1 (movie SEI)
            // and RDO/MBBRC at MSDK default — extra GPU time + a C2 hold.
            qsv.extend([
                "-gpb".into(),
                if lighting_host::session_policy::qsv_gpb() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-low_delay_brc".into(),
                if lighting_host::session_policy::qsv_low_delay_brc() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-mbbrc".into(),
                if lighting_host::session_policy::qsv_mbbrc() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-rdo".into(),
                if lighting_host::session_policy::qsv_rdo() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-adaptive_i".into(),
                if lighting_host::session_policy::qsv_adaptive_i() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-pic_timing_sei".into(),
                if lighting_host::session_policy::qsv_pic_timing_sei() {
                    "1".into()
                } else {
                    "0".into()
                },
                "-p_strategy".into(),
                lighting_host::session_policy::qsv_p_strategy().to_string(),
            ]);
        }
        qsv
    } else if encoder.contains("amf") {
        let mut amf = vec![
            "-quality".into(),
            "speed".into(),
            "-rc".into(),
            if settings.amf_rc.is_empty() {
                lighting_host::session_policy::amf_rc().into()
            } else {
                settings.amf_rc.clone()
            },
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
            "-refs".into(),
            lighting_host::session_policy::encoder_refs().to_string(),
            "-profile:v".into(),
            settings.profile.clone(),
            "-usage".into(),
            "ultralowlatency".into(),
            "-preanalysis".into(),
            "0".into(),
            "-preencode".into(),
            "0".into(),
            "-latency".into(),
            "1".into(),
            // ULL usage enables HRD unless this is explicit 0. Sunshine
            // `amd_enforce_hrd=disabled`. Unset is a 1-frame CPB hold.
            "-enforce_hrd".into(),
            if lighting_host::session_policy::amf_enforce_hrd() {
                "1".into()
            } else {
                "0".into()
            },
            "-filler_data".into(),
            if lighting_host::session_policy::amf_filler_data() {
                "1".into()
            } else {
                "0".into()
            },
            "-forced_idr".into(),
            if lighting_host::session_policy::amf_forced_idr() {
                "1".into()
            } else {
                "0".into()
            },
            "-async_depth".into(),
            lighting_host::session_policy::amf_async_depth().to_string(),
            "-vbaq".into(),
            "0".into(),
            // Same as NVENC/QSV: AMF SLICES_PER_FRAME follows avctx->slices
            // (0 = driver default, often >1 at 2K).
            "-slices".into(),
            lighting_host::session_policy::encoder_slices().to_string(),
        ];
        // AUD lets annexb cut the AU without waiting for Quiet or the
        // next VCL (NVENC/QSV already pass -aud 1). h264_amf and
        // hevc_amf both expose the key (AMF_VIDEO_ENCODER[_HEVC]_INSERT_AUD).
        if lighting_host::session_policy::amf_aud() {
            amf.extend(["-aud".into(), "1".into()]);
        }
        amf
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
            "-refs".into(),
            lighting_host::session_policy::encoder_refs().to_string(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-x265-params".into(),
            lighting_host::session_policy::x265_params(settings.fps.max(30)),
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
            "-refs".into(),
            lighting_host::session_policy::encoder_refs().to_string(),
            "-pix_fmt".into(),
            "yuv420p".into(),
            "-profile:v".into(),
            settings.profile.clone(),
            "-level:v".into(),
            level,
            "-x264-params".into(),
            lighting_host::session_policy::x264_params(
                settings.fps.max(30),
                avc_level(settings.width, settings.height, settings.fps),
            ),
        ]
    });
    flags
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
