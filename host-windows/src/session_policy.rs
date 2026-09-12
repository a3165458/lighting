/// After a client socket ends, keep the share session on the same listen
/// socket (and adb reverse) unless the user explicitly stopped sharing.
pub fn continue_accept_loop(user_stopped: bool) -> bool {
    !user_stopped
}

/// GDI `gdigrab` is last-resort only. BitBlt of an IddCx / game desktop is
/// typically 10–20 fps and misses exclusive Direct3D. Sunshine / Parsec use
/// DXGI Desktop Duplication whenever the output exists; we do the same and
/// keep GDI for monitors DXGI does not enumerate.
pub fn prefer_gdigrab_capture(_is_virtual: bool, has_dxgi: bool) -> bool {
    !has_dxgi
}

/// What to do to the PC panel after the tablet socket dies (sleep / power off).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientDropDesktopAction {
    None,
    ReassertPrimary,
    /// Win+P "仅第二屏幕" left the laptop detached. Undo it so the user is
    /// not staring at a black panel that only Win+Ctrl+Shift+B can revive.
    UndoExternal,
}

pub fn client_drop_desktop_action(
    tablet_only_active: bool,
    restore: PrimaryRestoreAction,
) -> ClientDropDesktopAction {
    if tablet_only_active {
        ClientDropDesktopAction::UndoExternal
    } else {
        match restore {
            PrimaryRestoreAction::Skip => ClientDropDesktopAction::None,
            _ => ClientDropDesktopAction::ReassertPrimary,
        }
    }
}

/// `WM_POWERBROADCAST` / `PowerRegisterSuspendResumeNotification` type:
/// `PBT_APMSUSPEND` (4). Hibernate uses the same event.
pub fn is_host_suspend_power_event(event: u32) -> bool {
    event == 4
}

/// Windows itself is about to sleep (Start menu / power button). Mirror and
/// extend leave the physical panel in the topology, so they must not call
/// SetDisplayConfig here. Tablet-only must undo `/external` *before* S3,
/// otherwise the panel wakes with backlight but no DWM (Ctrl+Win+Shift+B).
pub fn host_sleep_desktop_action(tablet_only_active: bool) -> ClientDropDesktopAction {
    if tablet_only_active {
        ClientDropDesktopAction::UndoExternal
    } else {
        ClientDropDesktopAction::None
    }
}

/// After the host wakes, do not immediately re-blank the PC. The lock screen
/// has to paint on a physical output. A new Start Share still applies 仅平板.
pub fn apply_tablet_only_on_hello(host_sleep_undid_external: bool) -> bool {
    !host_sleep_undid_external
}

/// GlideX / SuperDisplay run the virtual panel at 120 Hz even when the tablet
/// is 60 Hz. A 60 Hz IddCx mode makes DWM/DDA wait a 16 ms vsync; 120 Hz
/// cuts that wait in half. Never above 120: IddCx mode tables get sparse, and
/// higher values used to retime the laptop panel.
/// 120 Hz IddCx is for 90/120 Hz pads (GlideX). A 60 Hz 2020 tablet
/// (联想小新 Pad 2020) must stay at 60: bumping 60→120 is a mode change
/// that resets Desktop Duplication, encoder bootstrap fails, CONFIG never
/// ships, and the UI loops 适配平板分辨率 / 上一台已断开.
pub fn virtual_target_hz(_requested: u32, tablet_max: u32) -> u32 {
    if tablet_max >= 90 {
        120
    } else {
        60
    }
}

pub fn encoder_start_attempts() -> u32 {
    1
}

/// Mode change invalidates DXGI. One extra graph after settle, not the
/// old unbounded "抓屏未就绪，正在重试" loop.
pub fn encoder_start_attempts_after_mode_change() -> u32 {
    2
}

pub fn dda_settle_after_mode_change_ms() -> u64 {
    400
}

/// Size IddCx to the tablet panel after Hello so capture is 1:1 and the
/// pad SCALE_TO_FITs without letterbox. v0.1.62 skipped this because a
/// mode change plus ffmpeg writing a file named `0` meant CONFIG never
/// arrived. The encoder now settles DDA, Stop is polled, and ffmpeg
/// ends at `pipe:1`.
/// Hello 后再改 IddCx 会重置 DXGI，扩展屏退回 gdigrab（约 10–20 fps，
/// 声画一起卡）。虚拟屏用开始共享时的尺寸 1:1 抓取。
pub fn resize_virtual_display_after_hello() -> bool {
    false
}

/// 镜像不得 ChangeDisplaySettings 电脑主屏。2K 显示器会被切成 1080p。
pub fn mirror_changes_pc_mode() -> bool {
    false
}

/// The PC panel must never be the target of a virtual-display mode change.
pub fn is_safe_virtual_target(target_device: &str, primary_device: &str) -> bool {
    !target_device.is_empty()
        && !primary_device.is_empty()
        && !target_device.eq_ignore_ascii_case(primary_device)
}

/// How to put the host panel back without fighting a healthy extend desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaryRestoreAction {
    Skip,
    /// Size/Hz drifted but the laptop is still primary — do not CDS_SET_PRIMARY.
    TimingOnly,
    /// Physical panel is missing or no longer primary (the CAD / black-lid case).
    SetPrimary,
}

#[allow(clippy::too_many_arguments)]
pub fn primary_restore_action(
    current_device: Option<&str>,
    current_is_primary: bool,
    current_w: u32,
    current_h: u32,
    current_fps: u32,
    snap_device: &str,
    snap_w: u32,
    snap_h: u32,
    snap_fps: u32,
) -> PrimaryRestoreAction {
    match current_device {
        None => PrimaryRestoreAction::SetPrimary,
        Some(name) if !name.eq_ignore_ascii_case(snap_device) => PrimaryRestoreAction::SetPrimary,
        Some(_) if !current_is_primary => PrimaryRestoreAction::SetPrimary,
        Some(_) => {
            let size_drift = current_w != snap_w || current_h != snap_h;
            let hz_drift = snap_fps >= 30 && current_fps >= 30 && current_fps != snap_fps;
            if size_drift || hz_drift {
                PrimaryRestoreAction::TimingOnly
            } else {
                PrimaryRestoreAction::Skip
            }
        }
    }
}

/// Base wait before a client's Nth reconnect attempt (`fail_index` 0 = first retry).
/// The first slot is long enough that a reconnect does not stampede into host
/// teardown / `adb reverse` bounce; accept itself stays live concurrently.
pub fn reconnect_backoff_ms(fail_index: u32) -> u64 {
    match fail_index {
        0 => 650,
        1 => 900,
        2 => 1300,
        3 => 1800,
        _ => 2400,
    }
}

/// Bind + `adb reverse` before IddCx / UAC. Otherwise the tablet's USB
/// 127.0.0.1 connect is refused for the whole driver wait and its reconnect
/// budget expires while the host is still on 准备虚拟屏.
pub fn listen_before_virtual_prepare() -> bool {
    true
}

/// IddCx / UAC re-enumerates USB on Honor/Huawei. Reverse done *before*
/// the virtual display is then gone; `adb devices` still lists HA18C874
/// while 127.0.0.1:17400 is dead — same 「重连中 / 等待 USB」 hang.
pub fn refresh_usb_after_virtual_prepare() -> bool {
    true
}

/// Hellos that arrived while IddCx bounced USB are half-open on Honor.
/// Lenovo Xiaoxin Pad 2020 (and most AOSP pads) keep USB up — dropping a
/// live Hello then `adb reverse --remove` is the 重连中 loop.
/// Keep a Hello whose TCP still reads; only drop EOF/RST sockets.
pub fn drop_parked_hellos_after_virtual_prepare() -> bool {
    true
}

/// If a Hello survived VDD, do not `--remove` reverse or `am start`.
pub fn keep_live_hello_after_virtual_prepare() -> bool {
    true
}

/// Hello classify is 3s. An accept during VDD may still be reading Hello;
/// `--remove` then kills the in-flight connect on Lenovo.
pub fn skip_force_reverse_if_accept_newer_than_ms() -> u64 {
    3_000
}

pub fn should_force_reverse_after_virtual_prepare(
    has_live_hello: bool,
    last_accept_ms: Option<u64>,
) -> bool {
    if !force_usb_reverse_after_virtual_prepare() {
        return false;
    }
    if has_live_hello {
        return false;
    }
    if last_accept_ms.is_some_and(|ms| ms < skip_force_reverse_if_accept_newer_than_ms()) {
        return false;
    }
    true
}

/// Reverse must not wait on `dumpsys package`. The settings page already
/// probed the APK version; doing it again on the share path is the
/// HA18C874-visible / 127.0.0.1-dead hang.
pub fn usb_reverse_skips_package_probe() -> bool {
    true
}

/// `am start` after reverse is 20s × 4 on Honor. Bind + reverse are
/// already live; do not hold IddCx / accept behind the launch.
pub fn launch_stream_client_does_not_block_listen() -> bool {
    true
}

/// IddCx can drop reverse a few seconds after a successful `adb reverse`.
/// Host used to wait forever on video_rx with a dead 127.0.0.1 — tablet
/// stays on 重连中, host title still looks fine.
pub fn refresh_usb_while_waiting_for_hello() -> bool {
    true
}

pub fn verify_adb_reverse_list() -> bool {
    true
}

pub fn usb_wait_refresh_ms() -> u64 {
    2_000
}

/// 0.1.53 `adb reverse --remove` every 8s *and* `am start` every 2s.
/// Honor killed 127.0.0.1:17400 mid-connect and force-restarted the APK.
/// 0.1.54/55 then trusted `reverse --list`: after IddCx the list still
/// shows tcp:17400 while the tunnel is dead (HA18C874 visible, pad on
/// 重连中 / 没检测到电脑). Recreate at most this often, only while
/// still waiting for Hello — not a 2s launch heartbeat.
/// 0.1.56 always `--remove` + `am start` on this timer and chopped the
/// pad's in-flight 127.0.0.1 connect (重连中 loop). Skip when accept
/// recently proved the tunnel is live.
/// Must exceed the APK USB connect timeout (4s) plus one backoff: 0.1.57
/// used 4s, so every hung connect was `--remove`'d at the same instant
/// it would have timed out.
pub fn usb_reverse_recreate_after_ms() -> u64 {
    12_000
}

/// Honor adbd needs a beat after `reverse --remove` before bind works.
pub fn usb_reverse_settle_ms() -> u64 {
    250
}

/// IddCx re-enumerates USB. A listed reverse from before VDD is stale.
pub fn force_usb_reverse_after_virtual_prepare() -> bool {
    true
}

/// Re-`am start` only after reverse was actually missing. A 2s heartbeat
/// launch destroyed DisplayActivity on Honor/MagicOS.
pub fn relaunch_client_while_waiting_for_hello() -> bool {
    false
}

pub fn relaunch_client_when_reverse_restored() -> bool {
    true
}

/// 0.1.56 mapped force-recreate success to "restored" so every 4s
/// `am start` bounced DisplayActivity. The reconnect loop is already
/// running; `--remove` + launch is the 0.1.53 Honor 闪退 / 重连中 loop.
pub fn relaunch_client_after_forced_reverse_recreate() -> bool {
    false
}

/// `--remove` while classify_incoming is reading Hello kills the TCP
/// that just proved reverse is alive. Cover the 3s Hello timeout plus
/// one recreate tick.
pub fn usb_reverse_recreate_skips_recent_accept() -> bool {
    true
}

pub fn usb_reverse_recent_accept_ms() -> u64 {
    12_000
}

/// 0.1.57 `tokio::spawn(recreate_reverse_port)` after dropping the Hello
/// that IddCx just killed. The APK retries at 650ms and lands in
/// `--remove` — CONFIG never arrives, HUD stays 重连中. Rebuild on the
/// session task *before* waiting for the next Hello.
pub fn background_recreate_reverse_after_client() -> bool {
    false
}

pub fn sync_recreate_reverse_after_virtual_mode() -> bool {
    true
}

/// IddCx USB re-enum after ChangeDisplaySettingsEx. Recreate too early
/// binds a reverse that dies a second later.
pub fn virtual_mode_usb_settle_ms() -> u64 {
    800
}

/// One `am start` after the *synchronous* post-mode reverse is ready.
/// The APK skipBackoff-connects into a live tunnel. Not a 4s heartbeat.
pub fn relaunch_client_after_virtual_mode_reverse() -> bool {
    true
}

/// Whether wait-hello should `adb reverse --remove` + re-bind.
/// `last_recreate_ms` / `last_accept_ms` are elapsed times; `None` = never.
pub fn should_force_stale_reverse(
    hello_wait_ms: u64,
    last_recreate_ms: Option<u64>,
    last_accept_ms: Option<u64>,
) -> bool {
    let after = usb_reverse_recreate_after_ms();
    if after == 0 || hello_wait_ms < after {
        return false;
    }
    if last_recreate_ms.is_some_and(|ms| ms < after) {
        return false;
    }
    if usb_reverse_recreate_skips_recent_accept()
        && last_accept_ms.is_some_and(|ms| ms < usb_reverse_recent_accept_ms())
    {
        return false;
    }
    true
}

/// `restored_missing` = `ensure_reverse_port` added a mapping that was gone.
/// Forced recreate is a rebuild of a listed-but-maybe-stale tunnel.
pub fn should_relaunch_client_after_reverse(restored_missing: bool, forced_recreate: bool) -> bool {
    if forced_recreate {
        relaunch_client_after_forced_reverse_recreate()
    } else {
        restored_missing && relaunch_client_when_reverse_restored()
    }
}

/// ChangeDisplaySettingsEx on the IddCx panel re-enumerates USB on
/// Honor. Lenovo Xiaoxin Pad 2020 does not: the Hello TCP stays up.
/// Only abandon when the socket actually died (EOF/RST). Always dropping
/// a live Hello then `--remove` is the 重连中 loop on non-Honor pads.
pub fn rerequest_hello_after_virtual_mode_change() -> bool {
    true
}

pub fn should_abandon_hello_after_virtual_mode(did_change_mode: bool, socket_dead: bool) -> bool {
    did_change_mode && socket_dead && rerequest_hello_after_virtual_mode_change()
}

pub fn listen_port_from_bind(bind: &str) -> u16 {
    bind.rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .filter(|p: &u16| *p != 0)
        .unwrap_or(17400)
}

/// Tablet USB retry window. Must cover VDD + UAC (often 20-40s).
pub fn usb_reconnect_budget_ms() -> u64 {
    90_000
}

pub fn usb_reconnect_attempts() -> u32 {
    24
}

pub fn jitter_backoff_ms(base_ms: u64, jitter_ms: u64, max_jitter_ms: u64) -> u64 {
    base_ms.saturating_add(jitter_ms.min(max_jitter_ms))
}

/// USB 2.0 Hi-Speed practically carries ~25–35 MB/s. 25 Mbps video is ~3 MB/s,
/// so "not even 30 Hz" is not a USB bandwidth ceiling.
pub fn usb2_can_carry_bitrate_kbps(bitrate_kbps: u32) -> bool {
    bitrate_kbps <= 80_000
}

/// Encoded assembler→TCP-write queue. tokio `mpsc` cannot rendezvous
/// (min cap 1); that parked one 48 KB P while `write_all` flushed the
/// previous picture — one extra refresh vs GlideX. 0 is a std
/// `sync_channel` rendezvous: ffmpeg's pipe backs up one picture
/// earlier and ddagrab skips *input*. Do not `.max(1)` at the call
/// site. Recv lives on `lighting-video-write` so a ready AU is not
/// parked on spawn_blocking. IDR spikes wait on the socket instead of
/// hiding a GOP in RAM.
pub fn encoded_queue_capacity() -> usize {
    0
}

/// Drain extra encoded AUs into the same TCP write. After `recv` unblocks
/// the 1-deep queue the assembler may already have the next picture;
/// coalescing then parks the first AU for a whole encode and ~52 KB at
/// 25 Mbps/120 Hz exceeds the 48 KB send buffer. GlideX / Moonlight emit
/// one picture per send. PCM still rides the same write (not a second RTT).
pub fn coalesce_extra_video_on_write() -> bool {
    false
}

/// Raw annexb reader→assembler queue. 8 held ~8 AUs (60–130 ms at 120 Hz)
/// so ffmpeg never saw backpressure and a 1-deep encoded queue could
/// not make it skip *input* frames. 2 still parked the next AU's
/// Data+Quiet while `push_blocking` waited on the encoded send
/// — one extra refresh next to the laptop. 1 still parks one
/// chunk (a full 25 Mbps P at the 48 KB pipe) while the assembler
/// blocks on encoded-queue send. 0 is a rendezvous: the reader cannot
/// take the next ffmpeg write until the assembler has the current
/// chunk, so the pipe backs up one picture earlier and ddagrab skips
/// *input* instead of hiding a refresh. GlideX has no such hop.
pub fn annexb_raw_queue_capacity() -> usize {
    0
}

/// ffmpeg `-thread_queue_size`. One queued capture is ~16 ms at 60 Hz;
/// two was still a visible glass delay next to a real monitor.
pub fn capture_thread_queue_size() -> u32 {
    1
}

/// ffmpeg default `-filter_complex_threads` is nproc and the graph then
/// queues extra GPU frames (one hop per worker). GlideX/Sunshine native
/// capture has no such pool. Keep a single filter thread.
pub fn ffmpeg_filter_threads() -> u32 {
    1
}

/// ffmpeg default ON auto-inserts scale/format between filter pads when
/// pixfmts do not match. That inserted `scale_d3d11` hardcodes a 10-frame
/// pool (the graph we never emit). Identity NVENC then looks successful
/// while every picture sits in that pool — one extra refresh vs GlideX.
/// Off: mismatch fails over to the explicit CUDA/QSV/AMF convert with
/// extra_hw_frames=0.
pub fn ffmpeg_auto_conversion_filters() -> bool {
    false
}

/// ffmpeg `OPT_BOOL`. `-auto_conversion_filters 0` does **not** turn it
/// off: the `0` is parsed as the output URL (`Unable to find a suitable
/// output format for '0'`), every encoder graph dies before IDR, and the
/// pad stays on 重连中. Disable with `-noauto_conversion_filters`.
pub fn ffmpeg_auto_conversion_cli_arg() -> &'static str {
    if ffmpeg_auto_conversion_filters() {
        "-auto_conversion_filters"
    } else {
        "-noauto_conversion_filters"
    }
}

pub fn ffmpeg_auto_conversion_filter_args() -> [&'static str; 1] {
    [ffmpeg_auto_conversion_cli_arg()]
}

/// Which derived-device family a capture graph belongs to.
/// CPU `hwdownload` graphs return none so a failed CUDA init does not skip
/// the portable fallback.
pub fn graph_uses_hw_family(graph: &str, family: &str) -> bool {
    match family {
        "cuda" => {
            graph.contains("scale_cuda")
                || graph.contains("derive_device=cuda")
                || graph.contains("hwupload_cuda")
        }
        "qsv" => graph.contains("derive_device=qsv") || graph.contains("scale_qsv"),
        "amf" => graph.contains("vpp_amf") || graph.contains("derive_device=amf"),
        _ => false,
    }
}

/// ffmpeg stderr that means every remaining graph in this family will fail
/// the same way (device derive is unimplemented / missing). Surfaces and
/// rate-control retries cannot fix it.
pub fn skip_hw_family_after_stderr(stderr: &str) -> Option<&'static str> {
    let cuda = stderr.contains("cuda=cuda@capture") || stderr.contains("cuda@capture");
    if cuda
        && (stderr.contains("Function not implemented")
            || stderr.contains("Device creation failed: -40"))
    {
        return Some("cuda");
    }
    if (stderr.contains("qsv=qsv@capture") || stderr.contains("qsv@capture"))
        && (stderr.contains("Error creating a MFX session")
            || stderr.contains("Device creation failed: -1313558101"))
    {
        return Some("qsv");
    }
    if (stderr.contains("amf=amf@capture") || stderr.contains("amf@capture"))
        && (stderr.contains("No such device") || stderr.contains("AMF failed to initialise"))
    {
        return Some("amf");
    }
    None
}

/// Global ffmpeg boolean flags do not consume a following 0/1.
fn ffmpeg_opt_bool(flag: &str) -> bool {
    matches!(
        flag,
        "-auto_conversion_filters"
            | "-noauto_conversion_filters"
            | "-hide_banner"
            | "-an"
            | "-sn"
            | "-dn"
            | "-y"
            | "-n"
            | "-stdin"
            | "-nostdin"
    )
}

/// True when a stray numeric token would become ffmpeg's output URL.
/// `-analyzeduration 0` is a value (safe). `-auto_conversion_filters 0`
/// is a boolean plus a file named `0` (fatal).
pub fn ffmpeg_args_have_bare_numeric_output(args: &[String]) -> bool {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a.starts_with('-') && a != "-" {
            if ffmpeg_opt_bool(a) {
                i += 1;
                continue;
            }
            i += 2;
            continue;
        }
        if a.parse::<i32>().is_ok() {
            return true;
        }
        i += 1;
    }
    false
}

/// GlideX / SuperDisplay / Moonlight: pointer and HID never share the video
/// TCP stream. A second LIT1 connection on the same port with this Hello.role
/// is the control plane. Video AUs cannot HOL-block the pointer.
pub fn hello_is_control_plane(role: &str) -> bool {
    role.eq_ignore_ascii_case("control")
}

/// How often the host samples the OS pointer for the tablet overlay.
pub fn cursor_sample_interval_ms() -> u64 {
    1
}

/// ffmpeg hwupload/hwmap extra pool. Default is often 16 frames (~250 ms).
/// 0 = no extra (Sunshine native DDA has none). 1 then 2 if the graph
/// never emits IDR. Unkeyed filters still keep the 16-frame default.
///
/// vf_hwmap reverse reads `AVFilterContext.extra_hw_frames` (pool = 2+N).
/// That field is the generic AVFilter option in `avfilter_options[]`.
/// `avfilter_init_dict` applies graph-string keys via
/// `av_opt_set_dict2(ctx, SEARCH_CHILDREN)`, so
/// `hwmap=reverse=1:extra_hw_frames=N` is the correct CLI. ffmpeg has no
/// dedicated `-extra_hw_frames` flag: `opt_default` would set
/// `AVCodecContext` (decode side) and never reach vf_hwmap.
pub fn hw_extra_frames() -> u32 {
    0
}

/// Second pool size if extra=0 stalls ffmpeg before IDR.
pub fn hw_extra_frames_fallback() -> u32 {
    1
}

pub fn hw_extra_frame_attempts() -> Vec<u32> {
    let mut out = Vec::new();
    for n in [hw_extra_frames(), hw_extra_frames_fallback(), 2] {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// NVENC concurrent surfaces. Auto/default can be 8–32 frames of encoder delay.
/// Sunshine native uses 1. Try 1 first; ffmpeg on some GPUs needs 2.
pub fn nvenc_surfaces() -> u32 {
    1
}

/// Second NVENC surface count if a 1-surface encoder never emits IDR.
pub fn nvenc_surfaces_fallback() -> u32 {
    2
}

pub fn nvenc_surface_attempts() -> Vec<u32> {
    let a = nvenc_surfaces();
    let b = nvenc_surfaces_fallback();
    if a == b {
        vec![a]
    } else {
        vec![a, b]
    }
}

/// NVIDIA Spatial AQ is a quality pass. Sunshine keeps it off in ULL.
pub fn nvenc_spatial_aq() -> bool {
    false
}

/// Sunshine ULL sets `rcParams.lowDelayKeyFrameScale = 1`. ffmpeg only
/// writes the field when `-ldkfs` is non-zero; unset, NVENC keeps extra
/// CPB around every IDR even with `-tune ull` / `-zerolatency 1`.
pub fn nvenc_ldkfs() -> u32 {
    1
}

/// Sunshine ULL: `CBR_LOWDELAY_HQ`. Plain `cbr` can keep a 1-frame RC delay
/// even with `-zerolatency 1`.
pub fn nvenc_rc() -> &'static str {
    "cbr_ld_hq"
}

pub fn nvenc_rc_fallback() -> &'static str {
    "cbr"
}

pub fn nvenc_rc_attempts() -> Vec<&'static str> {
    let a = nvenc_rc();
    let b = nvenc_rc_fallback();
    if a == b {
        vec![a]
    } else {
        vec![a, b]
    }
}

/// ffmpeg `h264_amf` / `hevc_amf` default `async_depth` is 16 pictures.
pub fn amf_async_depth() -> u32 {
    1
}

/// Sunshine ULL: AMF `vbr_latency`. Plain `cbr` can keep a 1-frame RC
/// delay even with `-usage ultralowlatency` / `-latency 1`.
pub fn amf_rc() -> &'static str {
    "vbr_latency"
}

pub fn amf_rc_fallback() -> &'static str {
    "cbr"
}

pub fn amf_rc_attempts() -> Vec<&'static str> {
    let a = amf_rc();
    let b = amf_rc_fallback();
    if a == b {
        vec![a]
    } else {
        vec![a, b]
    }
}

/// QSV VDENC. Without `-low_power 1`, h264_qsv can keep a GPU frame in
/// flight even with `async_depth=1` (Sunshine ULL uses VDENC).
pub fn qsv_low_power() -> bool {
    true
}

/// QSV rate-distortion optimization. Default on; it is extra GPU encode
/// time, not a picture queue. At 1440p120 a 4 ms encode that becomes ~10 ms
/// misses the next vsync. Sunshine ULL leaves RDO off.
pub fn qsv_rdo() -> bool {
    false
}

/// QSV adaptive I-frame placement. MSDK default is on (`-adaptive_i -1`);
/// that is a scene-cut scan on every picture, same class of extra GPU time
/// as RDO. Sunshine ULL sets AdaptiveI/AdaptiveB off. `-bf 0` already
/// drops B frames; `-adaptive_b 0` stops the encoder looking for them.
pub fn qsv_adaptive_i() -> bool {
    false
}

/// QSV P-reference. `0` leaves MSDK default, which is often P-pyramid
/// (looks at future pictures — a frame of encode delay). Sunshine ULL
/// sets SIMPLE. `1` = simple, `2` = pyramid.
pub fn qsv_p_strategy() -> u32 {
    1
}

/// QSV picture-timing SEI. Default on; it is the movie-timing payload
/// annexb then drops. Generating it is extra encode work every picture.
/// Sunshine ffmpeg `h264_qsv` sets `pic_timing_sei=0`.
pub fn qsv_pic_timing_sei() -> bool {
    false
}

/// QSV `max_dec_frame_buffering`. Unset follows the level (4–16) even
/// with `-refs 1`, so Android holds a DPB of reconstructed pictures —
/// one refresh of glass. Sunshine ULL writes 1 (same as NVENC dpb_size).
pub fn qsv_max_dec_frame_buffering() -> u32 {
    1
}

/// QSV `forced_idr`. Without it, `-g` I-frames can be non-IDR and the
/// tablet's skipUntilKey never recovers a torn GOP.
pub fn qsv_forced_idr() -> bool {
    true
}

/// H.264-only QSV private options (`max_dec_frame_buffering`, `look_ahead`).
/// `hevc_qsv` rejects those two and ffmpeg then falls through to libx265
/// (45 fps software — a GOP of glass). HEVC DPB is already capped in
/// `hevc_sps::rewrite_low_latency`. `p_strategy` is HEVC-valid
/// (`QSV_OPTION_P_STRATEGY` in ffmpeg `qsvenc_hevc.c`).
pub fn qsv_h264_private_options(encoder: &str) -> bool {
    encoder.contains("qsv") && encoder.contains("h264")
}

/// HEVC QSV `gpb`. Default 1 still emits generalized P/B pictures when
/// `-bf 0`. Android treats those as B slices and holds a reorder frame —
/// one refresh of glass next to the laptop. Intel low-delay HEVC is
/// `-bf 0 -gpb 0`. Not valid on h264_qsv.
pub fn qsv_gpb() -> bool {
    false
}

/// HEVC QSV LowDelayBRC. Default -1 leaves a 1-frame RC hold; Sunshine
/// ULL turns it on. Valid on hevc_qsv (`QSV_OPTION_LOW_DELAY_BRC`).
pub fn qsv_low_delay_brc() -> bool {
    true
}

/// HEVC QSV MBBRC. Default -1 is extra per-MB work; Sunshine ULL off.
pub fn qsv_mbbrc() -> bool {
    false
}

/// hevc_qsv accepts `gpb` / `pic_timing_sei` / `rdo` / `mbbrc` /
/// `adaptive_i` / `low_delay_brc` / `p_strategy` (ffmpeg `qsvenc_hevc.c`).
/// Default `p_strategy=0` is MSDK P-pyramid (looks at a future picture —
/// one refresh of encode delay). Sunshine ULL sets SIMPLE (`1`).
pub fn qsv_hevc_private_options(encoder: &str) -> bool {
    encoder.contains("qsv") && encoder.contains("hevc")
}

/// QSV `scenario`. Default `unknown` lets MSDK pick archive/quality
/// buffering (extra GPU pictures in flight). Sunshine / Moonlight use
/// `gamestreaming` so the encoder does not hold a refresh for lookahead
/// BRC. Valid on h264_qsv and hevc_qsv (`QSV_OPTION_SCENARIO`).
pub fn qsv_scenario() -> &'static str {
    "gamestreaming"
}

/// QSV `-aud`. Default 0. annexb only flushes a VCL AU on Quiet (PeekNamedPipe
/// + up to 1 ms idle) or the next VCL. An AUD is the AU start; NVENC already
///   emits one so the assembler cuts without waiting. Valid on h264_qsv and
///   hevc_qsv.
pub fn qsv_aud() -> bool {
    true
}

/// AMF `-aud` (h264_amf and hevc_amf). Default unset. Without an AU
/// delimiter the assembler holds the VCL until Quiet (PeekNamedPipe) or
/// the next picture — up to IDLE_FLUSH (1 ms) on every AMD frame.
/// NVENC/QSV already insert AUD. FFmpeg has accepted this key on
/// hevc_amf since the original AMF encoder (2017); header_insertion_mode
/// is VPS/SPS/PPS repeat, not an AU cut.
pub fn amf_aud() -> bool {
    true
}

/// AMF `ENFORCE_HRD`. The `ultralowlatency` usage profile turns HRD on,
/// so unset (`-1`) still holds a reconstructed picture for the CPB —
/// one refresh vs the laptop. Sunshine `amd_enforce_hrd=disabled`.
/// Valid on h264_amf and hevc_amf (`AV_OPT_TYPE_BOOL`).
pub fn amf_enforce_hrd() -> bool {
    false
}

/// AMF filler NAL stuffing. Default -1 follows HRD/CBR and pads every
/// AU; Sunshine ULL sets 0.
pub fn amf_filler_data() -> bool {
    false
}

/// AMF `-forced_idr`. Default 0, so `-g` I-frames can be non-IDR and the
/// tablet never recovers a torn GOP. NVENC/QSV already force IDR.
pub fn amf_forced_idr() -> bool {
    true
}

/// ffmpeg encoder `-flags:v`. Global `-flags low_delay` sits before `-i`,
/// and h264_amf/hevc_amf default `flags=+loop` then clears LOW_DELAY.
/// amfenc `output_delay = max_b_frames + (LOW_DELAY ? 0 : 1)` — one extra
/// encoded picture every AMD frame. `+low_delay` adds the bit without
/// dropping loop filter. Sunshine sets `AV_CODEC_FLAG_LOW_DELAY`.
pub fn encoder_video_flags() -> &'static str {
    "+low_delay"
}

/// ffmpeg muxer `-max_interleave_delta`. Default is 10 seconds: with
/// `-an` the muxer still waits that window before flushing a lone
/// video AU when DTS gaps appear (ddagrab 8000 Hz poll vs 120 fps
/// encode). GlideX has no muxer. 0 emits each packet immediately.
pub fn ffmpeg_max_interleave_delta() -> i64 {
    0
}

/// h264_qsv `-a53cc`. Default 1 walks A/53 caption SEI on every picture.
/// ddagrab has none; NVENC already turns this off. Not an hevc_qsv option.
pub fn qsv_a53cc() -> bool {
    false
}

/// h264_qsv `-vcm`. Sunshine ULL uses Video Conferencing Mode so the
/// encoder does not hold a CBR picture. Windows ffmpeg exposes it
/// (`QSV_HAVE_VCM`); hevc_qsv does not. Unknown-option spawn fails into
/// the existing encoder fallback chain — do not send this on HEVC.
pub fn qsv_vcm() -> bool {
    true
}

/// QSV `-recovery_point_sei`. Default -1 leaves MSDK unspecified; some
/// drivers then emit recovery-point SEI (or treat the stream as intra-
/// refresh) and hold a picture. Sunshine ULL sets 0 on h264_qsv and
/// hevc_qsv. Valid on both; annexb already drops SEI NALs, but the
/// encoder still pays the hold if this stays on.
pub fn qsv_recovery_point_sei() -> bool {
    false
}

/// Encoder DPB / `num_ref_frames`. NVENC default follows the level (4–16);
/// Android then holds decoded pictures. Moonlight decoder-errata: 1.
pub fn encoder_refs() -> u32 {
    1
}

/// ffmpeg generic `-slices`. NVENC already passes 1. QSV default
/// `NumSlice=0` lets MSDK split a 2K picture into several VCL NALs;
/// annexb then flushes on the second VCL and the tablet paints a torn
/// AU until the rest arrives (one refresh of glass). AMF
/// `SLICES_PER_FRAME` follows `avctx->slices`.
pub fn encoder_slices() -> u32 {
    1
}

/// ffmpeg `h264_nvenc`/`hevc_nvenc` `-dpb_size`. Default 0 is "hardware
/// auto" and writes `maxNumRefFrames=0`, which NVENC fills from the level
/// (4–16) even when `-refs 1`. That reconstructed-frame pool is a frame of
/// encode delay Sunshine ULL does not pay. 1 matches `encoder_refs`.
pub fn nvenc_dpb_size() -> u32 {
    1
}

/// ffmpeg nvenc `-extra_sei`. Default 1 walks A/53 / extra SEI on every
/// picture. ddagrab has none; skip the scan.
pub fn nvenc_extra_sei() -> bool {
    false
}

/// ffmpeg nvenc `-a53cc`. Default 1. Desktop Duplication has no captions.
pub fn nvenc_a53cc() -> bool {
    false
}

/// libx265 still look-aheads unless these are explicit. `-tune zerolatency`
/// is not enough on some builds (rc-lookahead stays ~20 → a GOP of glass wait).
/// `aud=1` so annexb can cut the AU without Quiet / IDLE_FLUSH (1 ms).
pub fn x265_params(gop: u32) -> String {
    let keyint = gop.max(1);
    format!(
        "bframes=0:ref=1:no-open-gop:keyint={keyint}:min-keyint={keyint}:rc-lookahead=0:sync-lookahead=0:frame-threads=1:no-scenecut:repeat-headers=1:aud=1"
    )
}

/// libx264. `-tune zerolatency` turns on slice-based threading, which splits
/// each picture into several VCL NALs. Qualcomm OMX then paints the first
/// slice (top rows) and magenta-washes the rest. Force a single slice.
pub fn x264_params(gop: u32, level: &str) -> String {
    let keyint = gop.max(1);
    format!(
        "bframes=0:ref=1:open-gop=0:keyint={keyint}:min-keyint={keyint}:rc-lookahead=0:sync-lookahead=0:sliced-threads=0:threads=1:scenecut=0:repeat-headers=1:aud=1:level={level}"
    )
}

/// WASAPI shared-mode loopback buffer, 100-ns units. 20 ms underran the
/// 10 ms packets (沙哑); 50 ms was audible lag. 30 ms is the middle.
pub fn wasapi_buffer_hns() -> i64 {
    300_000
}

/// Host audio packets waiting for the next video AU. 2 + take-latest
/// dropped 40% of PCM at 60 Hz (10 ms kept per 16 ms picture).
pub fn audio_capture_queue() -> usize {
    6
}

/// Bluetooth / HDMI going away invalidates the WASAPI client. Keep
/// following the current default render device so the tablet still
/// gets PCM while any host app is playing.
pub fn audio_follow_default_render_device() -> bool {
    true
}

pub fn audio_loopback_retry_ms() -> u64 {
    300
}

pub fn audio_default_device_poll_ms() -> u64 {
    250
}

/// AUDCLNT / MMDevice codes that mean "reopen loopback", not "give up".
pub fn audio_hresult_is_device_lost(hr: u32) -> bool {
    const AUDCLNT: u32 = 0x8889_0000;
    const DEVICE_INVALIDATED: u32 = 0x8889_0004;
    const NOT_FOUND: u32 = 0x8007_0490;
    const GEN_FAILURE: u32 = 0x8007_001F;
    hr == DEVICE_INVALIDATED
        || hr == NOT_FOUND
        || hr == GEN_FAILURE
        || (hr & 0xFFFF_0000) == AUDCLNT
}

/// Wait this long for the tablet's second LIT1 socket before starting ffmpeg.
/// The Android client opens it right after Hello, so this is usually already
/// queued. Cursor/HID must not sit behind encoder startup.
pub fn control_attach_wait_ms() -> u64 {
    0
}

/// Bake the OS pointer into the video TCP stream only when the tablet asked
/// for an overlay but the control socket never showed up (old APK).
pub fn mux_cursor_on_video(cursor_overlay: bool, control_attached: bool) -> bool {
    cursor_overlay && !control_attached
}

/// Local overlay used to vanish past the surface mapping / on_display
/// margin. Keep the Windows pointer in the capture instead.
pub fn paint_cursor_overlay() -> bool {
    false
}

pub fn effective_cursor_overlay(hello_asks: bool) -> bool {
    paint_cursor_overlay() && hello_asks
}

pub fn draw_mouse_in_video(hello_cursor_overlay: bool) -> bool {
    !effective_cursor_overlay(hello_cursor_overlay)
}

/// Only hide the OS pointer when the tablet overlay is the one pointer.
pub fn hide_os_cursor_on_captured_display() -> bool {
    paint_cursor_overlay()
}

/// Windows pointer trails paint extra copies into the framebuffer.
pub fn suppress_mouse_trails_while_sharing() -> bool {
    true
}

/// TCP send buffer. Video+PCM now go in one write (~30 KB P at 25 Mbps/120 Hz,
/// ~44 KB at the UI max 40 Mbps). 24 KB was smaller than that coalesced AU,
/// so write_all blocked on a USB reverse ACK while the next picture sat in
/// the 1-deep encoded queue. 64 KB still hid a second default P-frame
/// (~26 KB) — one extra refresh next to the laptop. 48 KB fits one picture
/// up to 40 Mbps, not two at 25 Mbps. IDR still write_all's in chunks.
/// Tablet `LitSocket` recvBytes must match or the kernel hides the second P.
pub fn tcp_send_buffer_bytes() -> usize {
    48 * 1024
}

/// TCP recv buffer on the video socket (host side, mostly unused).
pub fn tcp_recv_buffer_bytes() -> usize {
    48 * 1024
}

/// Linux doubles `SO_RCVBUF` (bookkeeping); `getsockopt` returns that
/// doubled window. Requesting 48 KB then advertises ~96 KB — two 25 Mbps
/// P-frames after encoded/annexb queues are 1-deep. If `actual` is already
/// ~`want`, this stack did not double (the 24 KB stall). Return the next
/// `setsockopt` value so the advertised window is one picture.
pub fn tcp_clamp_kernel_buffer(want: usize, actual: usize) -> usize {
    if want == 0 {
        return 0;
    }
    if actual >= want.saturating_mul(2) {
        (want / 2).max(8 * 1024)
    } else {
        want
    }
}

/// Control-plane socket: cursor packets are tens of bytes.
pub fn tcp_control_buffer_bytes() -> usize {
    16 * 1024
}

/// Windows delayed ACK is 200 ms (ACK every two segments). A USB reverse
/// RTT then parks the next AU in the 1-deep encoded queue. 1 = ACK every
/// packet (`SIO_TCP_SET_ACK_FREQUENCY`). Tablet already re-arms Linux
/// TCP_QUICKACK on every read.
pub fn tcp_ack_every_packet() -> bool {
    true
}

/// One LIT1 TCP write. Header then payload as two write_all() on a
/// TCP_NODELAY USB socket is two packets and an extra reverse RTT.
pub fn lit1_encode(ty: u8, flags: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + payload.len());
    out.extend_from_slice(b"LIT1");
    out.push(ty);
    out.push(flags);
    out.push(0);
    out.push(0);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// ffmpeg `ddagrab` `dup_frames`. Duplicating up to the timer adds a wait that
/// a real monitor does not have. Unique desktop frames only.
pub fn ddagrab_duplicate_frames() -> bool {
    false
}

/// With passthrough output, ddagrab is the actual frame-rate limiter.
/// Disabling duplicate frames does not cap unique frames at the negotiated
/// decoder rate. A 30 fps recovery Hello must also slow capture to 30 fps.
pub fn dda_poll_hz(encode_fps: u32) -> u32 {
    encode_fps.clamp(1, 120)
}

/// Audio must never drain ahead of a video AU on the same TCP writer.
/// Keep chronological PCM, newest last. take-latest (1) left 10 ms of
/// audio per 16 ms picture at 60 Hz — underrun / 沙哑.
pub fn audio_packets_per_video_frame() -> usize {
    3
}

/// How many waiting 10 ms PCM packets may ride with one video AU.
pub fn audio_packets_to_send(pending: usize, max: usize) -> usize {
    if max == 0 {
        0
    } else {
        pending.min(max)
    }
}

/// Video recv_timeout used to skip PCM when the next AU was late.
pub fn audio_flush_on_video_timeout() -> bool {
    true
}

/// GlideX / SuperDisplay encode at the virtual panel (120 Hz), not the
/// tablet vsync. `min(decoder_max_fps)` used to pin a 60 Hz pad to a 16 ms
/// encode grid even after IddCx was already presenting every 8 ms.
pub fn encode_fps(_req_fps: u32, tablet_max: u32, dec_fps: u32, hw: bool) -> u32 {
    if !hw {
        return 45;
    }
    let cap = tablet_max
        .max(24)
        .min(if dec_fps == 0 { 120 } else { dec_fps })
        .min(120);
    if cap >= 90 {
        120
    } else {
        cap.min(60).max(24)
    }
}

/// Intended encoder/panel rate for GOP/VBV (never the 8000 Hz DXGI poll).
/// Not a muxer `-r`: current ffmpeg FATAL-rejects `-r` + `fps_mode
/// passthrough`; older builds VSYNC_CFR and hold every picture ~1/fps.
/// SPS rewrite drops 8000 Hz VUI; Android KEY_FRAME_RATE is 120.
pub fn ffmpeg_output_fps(encode_fps: u32) -> u32 {
    encode_fps.max(1)
}

/// Muxer `-r` would either fail spawn (new ffmpeg) or CFR-FIFO every
/// picture (old ffmpeg). Keep `-fps_mode passthrough` only.
pub fn ffmpeg_muxer_sets_output_fps() -> bool {
    false
}

/// Sunshine `D3DKMTSetProcessSchedulingPriorityClass(HIGH)`.
/// A foreground game otherwise starves Desktop Duplication / NVENC on the
/// same GPU, so the tablet sits a refresh behind the laptop. REALTIME can
/// freeze NVIDIA + HAGS; HIGH is the safe class.
pub fn gpu_scheduling_priority_high() -> bool {
    true
}

/// MMCSS ("Games", then "Pro Audio") on capture / cursor / pipe threads.
/// THREAD_PRIORITY_HIGHEST still sits on a 15.6 ms quanta when a game owns
/// the virtual panel; MMCSS is the 1 ms boost. Handle is not reverted:
/// the thread stays in the task until it exits.
pub fn mmcss_capture_threads() -> bool {
    true
}

/// ffmpeg.exe ddagrab/NVENC threads stay THREAD_PRIORITY_NORMAL inside
/// HIGH_PRIORITY_CLASS. Sunshine's in-process capture is CRITICAL; a
/// foreground game on MMCSS then wins the 15.6 ms quanta against those
/// NORMAL workers. Re-apply HIGHEST on the child's threads (MMCSS cannot
/// attach across processes).
pub fn boost_ffmpeg_child_threads() -> bool {
    true
}

/// Anonymous `CreatePipe` default is 4 KB. A 25 Mbps P-frame is ~26 KB, so
/// ffmpeg stdout used to land as many short `Read`s and PeekNamedPipe went
/// quiet between them. 256 KB hid ~10 pictures at 120 Hz; 64 KB still hid
/// a second P-frame after encoded/annexb queues were already 1-deep —
/// one extra refresh next to the laptop. 32 KB is smaller than a 40 Mbps
/// P (~42 KB, the UI max): n==buf.len() then Quiet truncated the AU
/// every picture. 48 KB matches `tcp_send_buffer_bytes` — one 40 Mbps P
/// plus PCM, not two default P-frames (~52 KB). IDR still uses the
/// n==buf.len() spin; 4 KB fragments every picture and Quiet-truncates.
pub fn ffmpeg_pipe_buffer_bytes() -> u32 {
    48 * 1024
}

/// Never drop a P-frame from a live GOP: the decoder would show 1 fps until the
/// next IDR. Block the encoder instead so ffmpeg skips *input* frames.
pub fn drop_encoded_p_on_backpressure() -> bool {
    false
}

/// Prefer the encoder that lives on the same GPU as the duplicated output.
/// Cross-adapter DDA→NVENC on a laptop is a common 15 fps path.
pub fn encoder_fallback_chain(codec: &str, vendor_id: u32) -> Vec<&'static str> {
    let hevc = codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265");
    match vendor_id {
        0x8086 => {
            if hevc {
                vec!["hevc_qsv", "hevc_nvenc", "hevc_amf", "libx265"]
            } else {
                vec!["h264_qsv", "h264_nvenc", "h264_amf", "libx264"]
            }
        }
        0x1002 => {
            if hevc {
                vec!["hevc_amf", "hevc_nvenc", "hevc_qsv", "libx265"]
            } else {
                vec!["h264_amf", "h264_nvenc", "h264_qsv", "libx264"]
            }
        }
        _ => {
            if hevc {
                vec!["hevc_nvenc", "hevc_qsv", "hevc_amf", "libx265"]
            } else {
                vec!["h264_nvenc", "h264_qsv", "h264_amf", "libx264"]
            }
        }
    }
}

/// Pin MttVDD to a discrete GPU when we know its DXGI name.
pub fn pick_vdd_gpu_name(adapters: &[(u32, String)]) -> String {
    for want in [0x10DEu32, 0x1002] {
        if let Some((_, name)) = adapters.iter().find(|(id, n)| *id == want && !n.is_empty()) {
            return name.clone();
        }
    }
    adapters
        .iter()
        .find(|(_, n)| !n.is_empty())
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| "Best GPU (Auto)".into())
}

/// Smooth heartbeat round-trips so the latency tile does not flicker on a single
/// slow reply. `prev` 0 means "no sample yet", so the first reading is taken raw.
pub fn smooth_latency_ms(prev: u32, sample: u32) -> u32 {
    if prev == 0 {
        sample
    } else {
        (prev.saturating_mul(3) + sample) / 4
    }
}

/// One picture of VBV, like Sunshine `nvenc_vbv_increase = 0`.
/// The old 400 kb floor was ~2 frames at 25 Mbps / 120 Hz (~16 ms).
pub fn vbv_bufsize_kb(bitrate_kbps: u32, fps: u32) -> u32 {
    let fps = fps.max(24);
    (bitrate_kbps / fps).max(1)
}

/// MttVDD per-resolution refresh tags. 120 first (GlideX); 90/60 if the
/// tablet / IddCx table cannot lock 120.
pub fn vdd_refresh_rates() -> &'static [u32] {
    &[120, 90, 60]
}

pub fn vdd_resolution_xml(width: u32, height: u32) -> String {
    let mut block = format!(
        "        <resolution>
            <width>{width}</width>
            <height>{height}</height>
"
    );
    for hz in vdd_refresh_rates() {
        block.push_str(&format!(
            "            <refresh_rate>{hz}</refresh_rate>
"
        ));
    }
    block.push_str(
        "        </resolution>
",
    );
    block
}

/// Official schema allows several `<refresh_rate>` tags per size. Stock XML
/// often lists only 60, so ChangeDisplaySettingsEx(120) fails and DWM stays
/// on a 16 ms grid.
pub fn ensure_vdd_xml_high_refresh(xml: &str, width: u32, height: u32) -> String {
    let mut xml = xml.to_string();
    if let Some(start) = xml.find("<global>") {
        if let Some(rel_end) = xml[start..].find("</global>") {
            let end = start + rel_end + "</global>".len();
            xml.replace_range(
                start..end,
                "<global>
        <g_refresh_rate>120</g_refresh_rate>
        <g_refresh_rate>90</g_refresh_rate>
        <g_refresh_rate>60</g_refresh_rate>
        <g_refresh_rate>144</g_refresh_rate>
    </global>",
            );
        }
    }
    xml = ensure_vdd_resolution_blocks_have_120(&xml);
    if !vdd_xml_has_resolution(&xml, width, height) {
        let entry = vdd_resolution_xml(width, height);
        if let Some(idx) = xml.find("</resolutions>") {
            xml.insert_str(idx, &entry);
        }
    }
    xml
}

fn vdd_xml_has_resolution(xml: &str, width: u32, height: u32) -> bool {
    let w_tag = format!("<width>{width}</width>");
    let h_tag = format!("<height>{height}</height>");
    let mut rest = xml;
    while let Some(res_start) = rest.find("<resolution>") {
        let after = &rest[res_start..];
        let Some(rel_end) = after.find("</resolution>") else {
            break;
        };
        let block = &after[..rel_end];
        if block.contains(&w_tag) && block.contains(&h_tag) {
            return true;
        }
        rest = &after[rel_end + "</resolution>".len()..];
    }
    false
}

fn ensure_vdd_resolution_blocks_have_120(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len() + 256);
    let mut rest = xml;
    while let Some(res_start) = rest.find("<resolution>") {
        out.push_str(&rest[..res_start]);
        let after = &rest[res_start..];
        let Some(rel_end) = after.find("</resolution>") else {
            out.push_str(rest);
            return out;
        };
        let block_end = rel_end + "</resolution>".len();
        out.push_str(&ensure_vdd_resolution_block_120(&after[..block_end]));
        rest = &after[block_end..];
    }
    out.push_str(rest);
    out
}

fn ensure_vdd_resolution_block_120(block: &str) -> String {
    if block.contains("<refresh_rate>120</refresh_rate>") {
        return block.to_string();
    }
    let Some(idx) = block.find("</height>") else {
        return block.to_string();
    };
    let at = idx + "</height>".len();
    let extra = "
            <refresh_rate>120</refresh_rate>
            <refresh_rate>90</refresh_rate>";
    format!("{}{}{}", &block[..at], extra, &block[at..])
}

/// I/O / pipe failures from a dropped tablet must not tear down the share.
pub fn is_client_disconnect(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("broken pipe")
        || e.contains("connection reset")
        || e.contains("connection aborted")
        || e.contains("eof")
        || e.contains("os error 10054")
        || e.contains("os error 104")
        || e.contains("os error 32")
        || e.contains("send video failed")
        || e.contains("send audio failed")
}

/// Encoder/capture failure is not a tablet drop. Re-accepting Hello then
/// retries capture forever (抓屏未就绪 / 重连中) and Stop cannot interrupt
/// the ffmpeg graph loop. Only socket death keeps the listen loop.
pub fn continue_accept_after_handle_client_err(err: &str) -> bool {
    is_client_disconnect(err)
}

/// Poll must not resurrect `running` after the user hit Stop. stop_share
/// used to clear only the flag; the next poll copied session.running back.
pub fn effective_share_running(user_stopped: bool, session_running: bool) -> bool {
    session_running && !user_stopped
}

/// Orient `(dw, dh)` so it matches the landscape/portrait of `(sw, sh)`.
pub fn orient_box(sw: u32, sh: u32, dw: u32, dh: u32) -> (u32, u32) {
    if sw >= sh {
        if dw >= dh {
            (dw, dh)
        } else {
            (dh, dw)
        }
    } else if dh >= dw {
        (dw, dh)
    } else {
        (dh, dw)
    }
}

/// 16:9 vs 16:10 differs by ~11%. Keep a 4% band so 1920×1080 and 2560×1440
/// stay in one family, while 1920×1200 is rejected on a 16:9 laptop panel.
pub fn aspects_compatible(w1: u32, h1: u32, w2: u32, h2: u32) -> bool {
    if w1 == 0 || h1 == 0 || w2 == 0 || h2 == 0 {
        return false;
    }
    let a = w1 as f64 / h1 as f64;
    let b = w2 as f64 / h2 as f64;
    let denom = a.max(b);
    if denom <= f64::EPSILON {
        return false;
    }
    (a - b).abs() / denom <= 0.04
}

/// Largest listed mode is almost always the panel's native timing.
pub fn native_panel_mode(modes: &[(u32, u32, u32)]) -> Option<(u32, u32, u32)> {
    modes
        .iter()
        .copied()
        .filter(|&(w, h, _)| w >= 16 && h >= 16)
        .max_by_key(|&(w, h, fps)| (w as u64 * h as u64, fps))
}

/// Pick a PC mode near the tablet without leaving the panel's native aspect.
///
/// A 1920×1200 (16:10) tablet on a 16:9 laptop must NOT pick 1920×1200: that
/// CVT scaled timing is what made “跟随平板” stutter after 1080p felt fine.
pub fn pick_closest_display_mode(
    modes: &[(u32, u32, u32)],
    target_w: u32,
    target_h: u32,
    min_fps: u32,
    native_w: u32,
    native_h: u32,
) -> Option<(u32, u32, u32)> {
    if modes.is_empty() || target_w == 0 || target_h == 0 {
        return None;
    }
    let min_fps = min_fps.max(30);
    let (nw, nh) = if native_w >= 16 && native_h >= 16 {
        (native_w, native_h)
    } else {
        native_panel_mode(modes)
            .map(|(w, h, _)| (w, h))
            .unwrap_or((target_w, target_h))
    };

    let same_aspect: Vec<(u32, u32, u32)> = modes
        .iter()
        .copied()
        .filter(|&(w, h, fps)| {
            w >= 16 && h >= 16 && fps >= min_fps && aspects_compatible(w, h, nw, nh)
        })
        .collect();
    let pool: &[(u32, u32, u32)] = if !same_aspect.is_empty() {
        &same_aspect
    } else {
        // No native-aspect mode at min_fps — still refuse foreign aspect.
        let any_aspect: Vec<(u32, u32, u32)> = modes
            .iter()
            .copied()
            .filter(|&(w, h, _)| w >= 16 && h >= 16 && aspects_compatible(w, h, nw, nh))
            .collect();
        if any_aspect.is_empty() {
            return None;
        }
        return pick_in_pool(&any_aspect, target_w, target_h);
    };
    pick_in_pool(pool, target_w, target_h)
}

fn pick_in_pool(pool: &[(u32, u32, u32)], target_w: u32, target_h: u32) -> Option<(u32, u32, u32)> {
    type ScoredMode = ((u128, u32), (u32, u32, u32));
    let mut best: Option<ScoredMode> = None;
    for &(w, h, fps) in pool {
        let dw = w.abs_diff(target_w) as u128;
        let dh = h.abs_diff(target_h) as u128;
        let size_score = if dw == 0 && dh == 0 {
            0u128
        } else {
            dw * dw + dh * dh + 1
        };
        let key = (size_score, u32::MAX - fps);
        match best {
            None => best = Some((key, (w, h, fps))),
            Some((prev_key, _)) if key < prev_key => best = Some((key, (w, h, fps))),
            _ => {}
        }
    }
    best.map(|(_, mode)| mode)
}

/// Whether switching the desktop to `candidate` is actually worth it.
///
/// Never move a 16:9 laptop onto 1920×1200 (16:10) just because the tablet is
/// 1920×1200 — GPU scaling that timing is the stutter the user reported.
/// Leaving an already-scaled 16:10 mode back to native 16:9 is allowed.
pub fn should_switch_desktop_mode(
    current: (u32, u32, u32),
    candidate: (u32, u32, u32),
    target_w: u32,
    target_h: u32,
    native_w: u32,
    native_h: u32,
) -> bool {
    let (cw, ch, cfps) = current;
    let (nw, nh, nfps) = candidate;
    if nw == cw && nh == ch {
        return false;
    }
    if cfps > 0 && nfps * 10 < cfps * 9 {
        return false;
    }
    let native_ok = native_w >= 16 && native_h >= 16;
    if native_ok && !aspects_compatible(nw, nh, native_w, native_h) {
        return false;
    }
    if native_ok
        && !aspects_compatible(cw, ch, native_w, native_h)
        && aspects_compatible(nw, nh, native_w, native_h)
    {
        return true;
    }
    let dist = |w: u32, h: u32| -> u64 {
        let dw = w.abs_diff(target_w) as u64;
        let dh = h.abs_diff(target_h) as u64;
        dw * dw + dh * dh
    };
    dist(nw, nh) < dist(cw, ch)
}

/// Aspect-preserving downscale into a box. Never upscales.
pub fn fit_resolution(src_w: u32, src_h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let mut w = src_w.max(2);
    let mut h = src_h.max(2);
    if w > max_w || h > max_h {
        let scale = (max_w as f64 / w as f64).min(max_h as f64 / h as f64);
        w = ((w as f64 * scale) as u32) & !1;
        h = ((h as f64 * scale) as u32) & !1;
    }
    (w.max(2), h.max(2))
}

/// Final encode size: always clamp to the tablet panel when known, then ResCap
/// ceiling, then decoder limit, then quality scale. Primary can be 2K/4K —
/// the stream must still fit the tablet.
#[allow(clippy::too_many_arguments)]
pub fn compute_encode_size(
    src_w: u32,
    src_h: u32,
    screen_w: u32,
    screen_h: u32,
    max_w: u32,
    max_h: u32,
    scale: f32,
    dec_w: u32,
    dec_h: u32,
) -> (u32, u32) {
    let (cap_w, cap_h) = orient_box(src_w, src_h, max_w.max(16), max_h.max(16));
    let (mut box_w, mut box_h) = if screen_w > 0 && screen_h > 0 {
        let (sw, sh) = orient_box(src_w, src_h, screen_w, screen_h);
        (sw.min(cap_w).max(16), sh.min(cap_h).max(16))
    } else {
        (cap_w, cap_h)
    };
    if dec_w > 0 && dec_h > 0 {
        let (dw, dh) = orient_box(src_w, src_h, dec_w, dec_h);
        box_w = box_w.min(dw.max(16));
        box_h = box_h.min(dh.max(16));
    }
    let scale = (scale as f64).clamp(0.35, 1.0);
    let out_w = ((box_w as f64 * scale) as u32).max(16);
    let out_h = ((box_h as f64 * scale) as u32).max(16);
    fit_resolution(src_w, src_h, out_w, out_h)
}

/// Even pixels only. MediaCodec `widthAlignment` is often 16, but that is a
/// stride hint: 1920×1080 is `isSizeSupported` on every Android decoder
/// Lighting targets. Flooring to 16 turned 1080 into 1072 / 2340 into 2336,
/// sizes IddCx/VDD do not advertise — ffmpeg then hits `scale_d3d11`
/// (hardcoded 10-frame GPU pool). GlideX encodes the even panel.
pub fn align_dim(v: u32, _alignment: u32) -> u32 {
    v.max(16) & !1
}

/// Macroblock-align the bitstream, without touching IddCx modes.
///
/// 1920×1080 / 1280×720 keep their standard `frame_crop`. A 5:3 tablet fitted
/// into 1080p becomes 1800×1080 (then 1792×1056) — Qualcomm OMX on this GSI
/// still 花屏s (magenta wash, only the top rows of the desktop readable).
/// Snap those near-1080p odd sizes to 1920×1080, which already works in mirror.
pub fn align_avc_macroblocks(w: u32, h: u32) -> (u32, u32) {
    if matches!(
        (w, h),
        (1920, 1080) | (1280, 720) | (1920, 1200) | (1920, 1152) | (2560, 1440)
    ) {
        return (w, h);
    }
    if w <= 1920 && h <= 1088 {
        return (1920, 1080);
    }
    ((w / 16).max(1) * 16, (h / 32).max(1) * 32)
}

/// When IddCx did not land on `wanted`, encode the captured desktop 1:1
/// if it still fits the decoder. Shrinking in the filter graph is the
/// scale_d3d11 pool; the tablet already SCALE_TO_FITs.
pub fn encode_keep_dda_identity(
    capture_w: u32,
    capture_h: u32,
    wanted_w: u32,
    wanted_h: u32,
    dec_w: u32,
    dec_h: u32,
) -> (u32, u32) {
    let cw = align_dim(capture_w, 2);
    let ch = align_dim(capture_h, 2);
    let ww = align_dim(wanted_w, 2);
    let wh = align_dim(wanted_h, 2);
    if cw == ww && ch == wh {
        return (cw, ch);
    }
    let (lim_w, lim_h) = orient_box(cw, ch, dec_w.max(16), dec_h.max(16));
    if cw <= lim_w && ch <= lim_h {
        return (cw, ch);
    }
    (ww, wh)
}

/// Virtual panel size that matches the encoder output, so `ddagrab` is 1:1.
///
/// Hello.alignment (often 16) used to be applied *after* IddCx was already
/// on the raw tablet timing. 2340×1080 then encoded as 2336×1080, which
/// turns on ffmpeg `scale_d3d11` (hardcoded 10-frame GPU pool — GlideX
/// native DDA has none). Quality scale < 1 used to keep a 2K desktop and
/// scale in the filter graph for the same reason.
#[allow(clippy::too_many_arguments)]
pub fn virtual_panel_size(
    tablet_w: u32,
    tablet_h: u32,
    max_w: u32,
    max_h: u32,
    scale: f32,
    dec_w: u32,
    dec_h: u32,
    alignment: u32,
) -> (u32, u32) {
    let src_w = tablet_w.max(16);
    let src_h = tablet_h.max(16);
    let (w, h) = compute_encode_size(
        src_w, src_h, tablet_w, tablet_h, max_w, max_h, scale, dec_w, dec_h,
    );
    (align_dim(w, alignment), align_dim(h, alignment))
}

/// Parse AC/DC lid-close action indices from `powercfg /q SCHEME_CURRENT SUB_BUTTONS`.
/// Looks up the LIDACTION block (language-independent GUID / alias), then the
/// current AC and DC index lines (`Index:` or `索引:`).
pub fn parse_lid_current_indices(text: &str) -> Option<(u32, u32)> {
    let lower = text.to_ascii_lowercase();
    let start = lower
        .find("lidaction")
        .or_else(|| lower.find("5ca83367-6e45-459f-a27b-476b1ee90026"))?;
    let rest = &text[start..];
    let rest_lower = rest.to_ascii_lowercase();
    let end = rest_lower
        .get(8..)
        .and_then(|s| s.find("power setting guid"))
        .map(|i| i + 8)
        .unwrap_or(rest.len());
    let block = &rest[..end];
    let mut vals = Vec::new();
    for line in block.lines() {
        let l = line.to_ascii_lowercase();
        if !(l.contains("index") || line.contains("索引")) {
            continue;
        }
        let Some(hex_at) = l.rfind("0x") else {
            continue;
        };
        let hex: String = l[hex_at + 2..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if let Ok(v) = u32::from_str_radix(&hex, 16) {
            vals.push(v);
        }
    }
    if vals.len() >= 2 {
        Some((vals[0], vals[1]))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnect_keeps_listening() {
        assert!(continue_accept_loop(false));
        assert!(!continue_accept_loop(true));
    }

    #[test]
    fn virtual_capture_uses_dda_when_dxgi_exists() {
        assert!(!prefer_gdigrab_capture(true, true));
        assert!(prefer_gdigrab_capture(true, false));
        assert!(prefer_gdigrab_capture(false, false));
        assert!(!prefer_gdigrab_capture(false, true));
    }

    #[test]
    fn virtual_mode_change_never_targets_primary() {
        assert!(is_safe_virtual_target(r"\\.\DISPLAY2", r"\\.\DISPLAY1"));
        assert!(!is_safe_virtual_target(r"\\.\DISPLAY1", r"\\.\DISPLAY1"));
        assert!(!is_safe_virtual_target("", r"\\.\DISPLAY1"));
    }

    #[test]
    fn first_reconnect_is_slower_than_teardown_stampede() {
        assert!(reconnect_backoff_ms(0) >= 600);
        assert!(reconnect_backoff_ms(0) < reconnect_backoff_ms(4));
        assert_eq!(jitter_backoff_ms(650, 80, 200), 730);
        assert_eq!(jitter_backoff_ms(650, 999, 200), 850);
        assert!(listen_before_virtual_prepare());
        assert!(refresh_usb_after_virtual_prepare());
        assert!(drop_parked_hellos_after_virtual_prepare());
        assert!(keep_live_hello_after_virtual_prepare());
        assert_eq!(skip_force_reverse_if_accept_newer_than_ms(), 3_000);
        assert!(!should_force_reverse_after_virtual_prepare(true, None));
        assert!(!should_force_reverse_after_virtual_prepare(
            false,
            Some(500)
        ));
        assert!(should_force_reverse_after_virtual_prepare(false, None));
        assert!(should_force_reverse_after_virtual_prepare(
            false,
            Some(8_000)
        ));
        assert!(usb_reverse_skips_package_probe());
        assert!(launch_stream_client_does_not_block_listen());
        assert!(refresh_usb_while_waiting_for_hello());
        assert!(verify_adb_reverse_list());
        assert!(usb_wait_refresh_ms() >= 1_000 && usb_wait_refresh_ms() <= 5_000);
        assert!(usb_reverse_recreate_after_ms() >= 8_000);
        assert!(usb_reverse_recreate_after_ms() <= 20_000);
        assert!(usb_reverse_settle_ms() > 0 && usb_reverse_settle_ms() <= 1_000);
        assert!(force_usb_reverse_after_virtual_prepare());
        assert!(!relaunch_client_while_waiting_for_hello());
        assert!(relaunch_client_when_reverse_restored());
        assert!(!relaunch_client_after_forced_reverse_recreate());
        assert!(!background_recreate_reverse_after_client());
        assert!(sync_recreate_reverse_after_virtual_mode());
        assert!(virtual_mode_usb_settle_ms() >= 400);
        assert!(relaunch_client_after_virtual_mode_reverse());
        assert!(usb_reverse_recreate_skips_recent_accept());
        assert!(usb_reverse_recent_accept_ms() >= usb_reverse_recreate_after_ms());
        assert!(rerequest_hello_after_virtual_mode_change());
        // Lenovo Xiaoxin Pad: USB stays up — keep this Hello.
        assert!(!should_abandon_hello_after_virtual_mode(true, false));
        // Honor-style RST after IddCx — wait for a new Hello.
        assert!(should_abandon_hello_after_virtual_mode(true, true));
        assert!(!should_abandon_hello_after_virtual_mode(false, true));
        // 4s is the APK connect timeout — must not --remove on that beat.
        assert!(!should_force_stale_reverse(4_000, None, None));
        assert!(!should_force_stale_reverse(2_000, None, None));
        // No Hello and no accept past the recreate interval — rebuild.
        assert!(should_force_stale_reverse(12_000, None, None));
        // Accept in flight (Hello still being classified) — do not --remove.
        assert!(!should_force_stale_reverse(12_000, None, Some(500)));
        assert!(should_force_stale_reverse(
            24_000,
            Some(12_000),
            Some(13_000)
        ));
        // Last recreate was 2s ago — wait the interval.
        assert!(!should_force_stale_reverse(14_000, Some(2_000), None));
        assert!(should_relaunch_client_after_reverse(true, false));
        assert!(!should_relaunch_client_after_reverse(false, false));
        assert!(!should_relaunch_client_after_reverse(true, true));
        assert_eq!(listen_port_from_bind("0.0.0.0:17400"), 17400);
        assert_eq!(listen_port_from_bind("127.0.0.1:17400"), 17400);
        assert_eq!(listen_port_from_bind(""), 17400);
        assert!(usb_reconnect_budget_ms() >= 60_000);
        assert!(usb_reconnect_attempts() >= 12);
    }

    #[test]
    fn latency_settles_instead_of_jumping() {
        assert_eq!(smooth_latency_ms(0, 28), 28);
        assert_eq!(smooth_latency_ms(28, 28), 28);
        let spiked = smooth_latency_ms(28, 400);
        assert!(spiked > 28 && spiked < 400);
    }

    #[test]
    fn vbv_targets_about_one_frame() {
        // 25 Mbps @ 60fps → ~416 kb for 1 frame; @120 → 208 kb, not the old 400 kb floor.
        assert_eq!(vbv_bufsize_kb(25_000, 60), 416);
        assert_eq!(vbv_bufsize_kb(25_000, 120), 208);
        assert_eq!(vbv_bufsize_kb(8_000, 120), 66);
        assert!(vbv_bufsize_kb(40_000, 30) <= 40_000);
        assert!(vbv_bufsize_kb(25_000, 60) < 25_000 / 2);
    }

    #[test]
    fn vdd_xml_adds_120hz_to_60hz_stock_modes() {
        let xml = r#"<vdd_settings>
    <global>
        <g_refresh_rate>60</g_refresh_rate>
    </global>
    <resolutions>
        <resolution>
            <width>1920</width>
            <height>1080</height>
            <refresh_rate>60</refresh_rate>
        </resolution>
    </resolutions>
</vdd_settings>"#;
        let out = ensure_vdd_xml_high_refresh(xml, 2560, 1600);
        assert!(out.contains("<g_refresh_rate>120</g_refresh_rate>"));
        assert!(out.contains("<width>1920</width>"));
        assert!(out.contains("<refresh_rate>120</refresh_rate>"));
        assert!(out.contains("<width>2560</width>"));
        assert!(out.contains("<height>1600</height>"));
        assert_eq!(vdd_refresh_rates(), &[120, 90, 60]);
    }

    #[test]
    fn control_hello_is_not_a_video_session() {
        assert!(hello_is_control_plane("control"));
        assert!(hello_is_control_plane("Control"));
        assert!(!hello_is_control_plane(""));
        assert!(!hello_is_control_plane("stream"));
    }

    #[test]
    fn capture_queue_is_single_frame() {
        assert_eq!(capture_thread_queue_size(), 1);
        assert_eq!(ffmpeg_filter_threads(), 1);
        assert!(!ffmpeg_auto_conversion_filters());
        assert_eq!(
            ffmpeg_auto_conversion_filter_args(),
            ["-noauto_conversion_filters"]
        );
        assert_eq!(
            ffmpeg_auto_conversion_cli_arg(),
            "-noauto_conversion_filters"
        );
        assert!(ffmpeg_args_have_bare_numeric_output(&[
            "-auto_conversion_filters".into(),
            "0".into(),
            "pipe:1".into(),
        ]));
        assert!(!ffmpeg_args_have_bare_numeric_output(&[
            "-noauto_conversion_filters".into(),
            "-analyzeduration".into(),
            "0".into(),
            "-f".into(),
            "h264".into(),
            "pipe:1".into(),
        ]));
        assert_eq!(cursor_sample_interval_ms(), 1);
        assert!(gpu_scheduling_priority_high());
        assert!(mmcss_capture_threads());
        assert!(boost_ffmpeg_child_threads());
        assert_eq!(hw_extra_frames(), 0);
        assert_eq!(hw_extra_frames_fallback(), 1);
        assert_eq!(hw_extra_frame_attempts(), vec![0, 1, 2]);
        assert_eq!(nvenc_surfaces(), 1);
        assert_eq!(nvenc_surfaces_fallback(), 2);
        assert_eq!(nvenc_rc(), "cbr_ld_hq");
        assert_ne!(nvenc_rc(), "cbr");
        assert_eq!(nvenc_rc_attempts(), vec!["cbr_ld_hq", "cbr"]);
        assert_eq!(amf_async_depth(), 1);
        assert_eq!(amf_rc(), "vbr_latency");
        assert_ne!(amf_rc(), "cbr");
        assert_eq!(amf_rc_attempts(), vec!["vbr_latency", "cbr"]);
        assert!(qsv_low_power());
        assert!(!qsv_rdo());
        assert!(!qsv_adaptive_i());
        assert_eq!(qsv_p_strategy(), 1);
        assert!(!qsv_pic_timing_sei());
        assert_eq!(qsv_max_dec_frame_buffering(), 1);
        assert!(qsv_forced_idr());
        assert!(qsv_h264_private_options("h264_qsv"));
        assert!(!qsv_h264_private_options("hevc_qsv"));
        assert!(!qsv_h264_private_options("h264_nvenc"));
        assert!(!qsv_gpb());
        assert!(qsv_low_delay_brc());
        assert!(!qsv_mbbrc());
        assert!(qsv_hevc_private_options("hevc_qsv"));
        assert!(!qsv_hevc_private_options("h264_qsv"));
        assert!(!qsv_hevc_private_options("hevc_nvenc"));
        assert_eq!(qsv_scenario(), "gamestreaming");
        assert!(qsv_aud());
        assert!(amf_aud());
        assert!(!amf_enforce_hrd());
        assert!(!amf_filler_data());
        assert!(amf_forced_idr());
        assert_eq!(encoder_video_flags(), "+low_delay");
        assert_eq!(ffmpeg_max_interleave_delta(), 0);
        assert!(!qsv_a53cc());
        assert!(qsv_vcm());
        assert!(!qsv_recovery_point_sei());
        assert_eq!(encoder_refs(), 1);
        assert_eq!(encoder_slices(), 1);
        assert_eq!(nvenc_dpb_size(), 1);
        assert!(!nvenc_extra_sei());
        assert!(!nvenc_a53cc());
        assert!(x265_params(120).contains("rc-lookahead=0"));
        assert!(x265_params(120).contains("bframes=0"));
        assert!(x265_params(120).contains("keyint=120"));
        assert!(x265_params(120).contains("aud=1"));
        assert!(x264_params(120, "4.2").contains("bframes=0"));
        assert!(x264_params(120, "4.2").contains("sliced-threads=0"));
        assert!(x264_params(120, "4.2").contains("threads=1"));
        assert!(!x264_params(120, "4.2").contains("sliced-threads=1"));
        assert!(x264_params(120, "4.2").contains("aud=1"));
        assert!(x264_params(120, "4.2").contains("keyint=120"));
        assert!(x264_params(120, "4.2").contains("level=4.2"));
        assert_eq!(nvenc_surface_attempts(), vec![1, 2]);
        assert!(!nvenc_spatial_aq());
        assert_eq!(nvenc_ldkfs(), 1);
        assert_eq!(wasapi_buffer_hns(), 300_000);
        assert_eq!(audio_capture_queue(), 6);
        assert!(audio_follow_default_render_device());
        assert!(audio_loopback_retry_ms() >= 100);
        assert!(audio_default_device_poll_ms() >= 100);
        assert!(audio_hresult_is_device_lost(0x8889_0004));
        assert!(audio_hresult_is_device_lost(0x8889_0010));
        assert!(audio_hresult_is_device_lost(0x8007_0490));
        assert!(!audio_hresult_is_device_lost(0));
        assert!(!ddagrab_duplicate_frames());
        assert_eq!(dda_poll_hz(60), 60);
        assert_eq!(dda_poll_hz(120), 120);
        assert_eq!(dda_poll_hz(144), 120);
        assert_eq!(dda_poll_hz(30), 30);
        assert_eq!(tcp_send_buffer_bytes(), 48 * 1024);
        assert!(tcp_control_buffer_bytes() < tcp_send_buffer_bytes());
        // One default P+PCM (~28 KB) and one 40 Mbps P (~42 KB) fit;
        // two default P-frames (~52 KB) do not.
        let p25 = 25_000 * 1000 / 8 / 120;
        let p40 = 40_000 * 1000 / 8 / 120;
        assert!(tcp_send_buffer_bytes() > p25 + 8 * 1024);
        assert!(tcp_send_buffer_bytes() > p40);
        assert!(tcp_send_buffer_bytes() < 2 * p25);
        assert_eq!(tcp_clamp_kernel_buffer(48 * 1024, 48 * 1024), 48 * 1024);
        assert_eq!(tcp_clamp_kernel_buffer(48 * 1024, 96 * 1024), 24 * 1024);
        assert_eq!(tcp_clamp_kernel_buffer(48 * 1024, 64 * 1024), 48 * 1024);
        assert_eq!(tcp_clamp_kernel_buffer(16 * 1024, 32 * 1024), 8 * 1024);
        assert!(tcp_ack_every_packet());
        let frame = lit1_encode(3, 1, &[9, 8, 7]);
        assert_eq!(&frame[..4], b"LIT1");
        assert_eq!(frame[4], 3);
        assert_eq!(frame[5], 1);
        assert_eq!(&frame[8..12], &3u32.to_be_bytes());
        assert_eq!(&frame[12..], &[9, 8, 7]);
        assert_eq!(lit1_encode(5, 0, &[]).len(), 12);
    }

    #[test]
    fn encode_fps_matches_virtual_120_like_glidex() {
        assert_eq!(encode_fps(60, 120, 120, true), 120);
        assert_eq!(encode_fps(60, 90, 60, true), 60);
        assert_eq!(encode_fps(60, 120, 60, false), 45);
        // 联想小新 Pad 2020 is 60 Hz / SD662 — do not encode 120.
        assert_eq!(encode_fps(30, 60, 60, true), 60);
        assert_eq!(encode_fps(60, 60, 30, true), 30);
        assert_eq!(ffmpeg_output_fps(encode_fps(60, 60, 60, true)), 60);
        assert_eq!(ffmpeg_output_fps(120), dda_poll_hz(120));
        assert_eq!(ffmpeg_output_fps(45), 45);
        assert!(!ffmpeg_muxer_sets_output_fps());
        assert_eq!(ffmpeg_pipe_buffer_bytes(), 48 * 1024);
        assert_eq!(ffmpeg_pipe_buffer_bytes() as usize, tcp_send_buffer_bytes());
        let pipe_p25 = 25_000 * 1000 / 8 / 120;
        let pipe_p40 = 40_000 * 1000 / 8 / 120;
        assert!(ffmpeg_pipe_buffer_bytes() as usize > pipe_p40);
        assert!((ffmpeg_pipe_buffer_bytes() as usize) < 2 * pipe_p25);
        assert!(ffmpeg_pipe_buffer_bytes() > 16 * 1024);
        // annexb read vec must be this size: larger and n==buf.len() never
        // fires, so a full-pipe IDR Quiet-flushes a truncated slice.
        assert_eq!(audio_packets_per_video_frame(), 3);
        assert_eq!(audio_packets_to_send(5, 3), 3);
        assert_eq!(audio_packets_to_send(1, 3), 1);
        assert_eq!(audio_packets_to_send(5, 0), 0);
        assert!(audio_flush_on_video_timeout());
        assert_eq!(control_attach_wait_ms(), 0);
        assert!(mux_cursor_on_video(true, false));
        assert!(!mux_cursor_on_video(true, true));
        assert!(!mux_cursor_on_video(false, false));
        assert!(!paint_cursor_overlay());
        assert!(!effective_cursor_overlay(true));
        assert!(draw_mouse_in_video(true));
        assert!(draw_mouse_in_video(false));
        assert!(!hide_os_cursor_on_captured_display());
        assert!(suppress_mouse_trails_while_sharing());
    }

    #[test]
    fn quality_baseline_defaults_are_not_reduced() {
        // Guardrail for the latency goal: do not "win" latency by cutting
        // the UI defaults users start from (100% / 60fps / 25Mbps).
        let defaults = crate::view::Settings::default();
        assert_eq!(defaults.quality_pct, 100);
        assert_eq!(defaults.fps, 60);
        assert_eq!(defaults.bitrate_kbps, 25_000);
    }

    #[test]
    fn classifies_common_disconnects() {
        assert!(is_client_disconnect("send video failed: broken pipe"));
        assert!(is_client_disconnect("connection reset by peer"));
        assert!(is_client_disconnect("early eof"));
        assert!(is_client_disconnect("os error 10054"));
        assert!(!is_client_disconnect("所选显示器不存在"));
        assert!(!is_client_disconnect("找不到 ffmpeg"));
        assert!(continue_accept_after_handle_client_err(
            "send video failed: broken pipe"
        ));
        assert!(!continue_accept_after_handle_client_err(
            "encoder restart did not emit codec-config + IDR in time"
        ));
        assert!(!continue_accept_after_handle_client_err("抓屏启动失败"));
        assert!(effective_share_running(false, true));
        assert!(!effective_share_running(true, true));
        assert!(!effective_share_running(true, false));
        assert!(!effective_share_running(false, false));
    }

    #[test]
    fn picks_exact_tablet_mode_at_highest_refresh() {
        let modes = [
            (1920, 1080, 60),
            (2560, 1440, 60),
            (2000, 1200, 60),
            (2000, 1200, 120),
        ];
        // Native 16:10 panel: exact tablet size at the higher refresh.
        assert_eq!(
            pick_closest_display_mode(&modes, 2000, 1200, 60, 2000, 1200),
            Some((2000, 1200, 120))
        );
    }

    #[test]
    fn picks_nearest_when_exact_missing() {
        let modes = [(3840, 2160, 60), (2560, 1440, 60), (1920, 1080, 60)];
        assert_eq!(
            pick_closest_display_mode(&modes, 2000, 1200, 60, 3840, 2160),
            Some((1920, 1080, 60))
        );
    }

    #[test]
    fn keeps_high_refresh_over_closer_size() {
        let modes = [(2000, 1200, 60), (1920, 1200, 165)];
        assert_eq!(
            pick_closest_display_mode(&modes, 2000, 1200, 120, 1920, 1200),
            Some((1920, 1200, 165))
        );
    }

    #[test]
    fn sixteen_ten_tablet_stays_on_sixteen_nine_1080() {
        // User: tablet 1920×1200, 1080p felt fine, follow-tablet 1200p stuttered.
        let modes = [
            (2560, 1440, 60),
            (1920, 1080, 60),
            (1920, 1200, 60),
            (1280, 800, 60),
        ];
        assert_eq!(
            pick_closest_display_mode(&modes, 1920, 1200, 60, 2560, 1440),
            Some((1920, 1080, 60))
        );
        assert!(!aspects_compatible(1920, 1080, 1920, 1200));
        assert!(aspects_compatible(1920, 1080, 2560, 1440));
    }

    #[test]
    fn refuses_1080_to_1200_on_sixteen_nine_panel() {
        assert!(!should_switch_desktop_mode(
            (1920, 1080, 60),
            (1920, 1200, 60),
            1920,
            1200,
            2560,
            1440
        ));
    }

    #[test]
    fn restores_scaled_1200_back_to_native_aspect_1080() {
        assert!(should_switch_desktop_mode(
            (1920, 1200, 60),
            (1920, 1080, 60),
            1920,
            1200,
            2560,
            1440
        ));
    }

    #[test]
    fn refuses_switch_that_costs_refresh() {
        assert!(!should_switch_desktop_mode(
            (2560, 1440, 165),
            (1920, 1080, 60),
            1920,
            1200,
            2560,
            1440
        ));
        // Same 16:9 family, same refresh, closer to tablet → 1080p is OK.
        assert!(should_switch_desktop_mode(
            (2560, 1440, 60),
            (1920, 1080, 60),
            1920,
            1200,
            2560,
            1440
        ));
        // 16:10 1200p is never OK on a 16:9 panel.
        assert!(!should_switch_desktop_mode(
            (2560, 1440, 60),
            (1920, 1200, 60),
            1920,
            1200,
            2560,
            1440
        ));
    }

    #[test]
    fn refuses_switch_that_is_not_closer() {
        assert!(!should_switch_desktop_mode(
            (1920, 1080, 60),
            (1920, 1080, 60),
            1920,
            1200,
            2560,
            1440
        ));
        assert!(!should_switch_desktop_mode(
            (1920, 1080, 60),
            (1280, 720, 60),
            1920,
            1200,
            2560,
            1440
        ));
    }

    #[test]
    fn mirror_2k_desktop_fits_non_2k_tablet() {
        // User bug: 2K primary mirrored to a 1920×1200 tablet must not stay 2560×1440,
        // even when ResCap is「最高 2K」and the decoder claims 4K.
        let (w, h) = compute_encode_size(2560, 1440, 1920, 1200, 2560, 1440, 1.0, 3840, 2160);
        assert!(w <= 1920 && h <= 1200, "{w}×{h}");
        assert_eq!((w, h), (1920, 1080));
    }

    #[test]
    fn res_cap_fhd_still_clamps_below_tablet() {
        let (w, h) = compute_encode_size(3840, 2160, 2560, 1600, 1920, 1080, 1.0, 3840, 2160);
        assert!(w <= 1920 && h <= 1080, "{w}×{h}");
    }

    #[test]
    fn missing_screen_falls_back_to_res_cap() {
        let (w, h) = compute_encode_size(2560, 1440, 0, 0, 1920, 1080, 1.0, 3840, 2160);
        assert!(w <= 1920 && h <= 1080, "{w}×{h}");
    }

    #[test]
    fn virtual_panel_keeps_even_panel_not_16_floor() {
        // 16-floor used to invent 2336×1072, which IddCx does not list.
        let (w, h) = virtual_panel_size(2340, 1080, 3840, 2160, 1.0, 3840, 2160, 16);
        assert_eq!((w, h), (2340, 1080));
        assert_eq!(align_dim(2340, 16), 2340);
        assert_eq!(align_dim(1080, 16), 1080);
        assert_eq!(align_dim(1920, 16), 1920);
    }

    #[test]
    fn avc_macroblock_align_fixes_1800x1080_huaping() {
        assert_eq!(align_avc_macroblocks(1800, 1080), (1920, 1080));
        assert_eq!(align_avc_macroblocks(1792, 1056), (1920, 1080));
        assert_eq!(align_avc_macroblocks(1792, 1072), (1920, 1080));
        assert_eq!(align_avc_macroblocks(1920, 1080), (1920, 1080));
        assert_eq!(align_avc_macroblocks(1280, 720), (1280, 720));
        assert_eq!(align_avc_macroblocks(1920, 1152), (1920, 1152));
    }

    #[test]
    fn capture_identity_wins_when_vdd_misses_tablet_mode() {
        // VDD stayed 2560×1440; decoder is 4K. Do not scale_d3d11 to 1080p.
        let (w, h) = encode_keep_dda_identity(2560, 1440, 1920, 1080, 3840, 2160);
        assert_eq!((w, h), (2560, 1440));
        assert!(!crate::capture_graph::needs_scale(2560, 1440, w, h));
    }

    #[test]
    fn capture_identity_still_clamps_when_decoder_is_smaller() {
        let (w, h) = encode_keep_dda_identity(2560, 1440, 1920, 1080, 1920, 1088);
        assert_eq!((w, h), (1920, 1080));
    }

    #[test]
    fn odd_iddcx_1152_stays_until_hello_advertises_1080p() {
        // 联想小新 Pad 2020: VDD 1920x1152 fits a 1920x1920 capability box
        // and Qualcomm then fails configure(). Keep identity until the next
        // Hello actually lowers the decoder ceiling.
        let (w, h) = encode_keep_dda_identity(1920, 1152, 1920, 1200, 1920, 1920);
        assert_eq!((w, h), (1920, 1152));
        let (w, h) = encode_keep_dda_identity(1920, 1152, 1920, 1080, 1920, 1080);
        assert_eq!((w, h), (1920, 1080));
    }

    #[test]
    fn vdd_xml_inserts_paired_resolution_not_loose_tags() {
        let xml = r#"<vdd_settings>
    <resolutions>
        <resolution>
            <width>1920</width>
            <height>1080</height>
            <refresh_rate>60</refresh_rate>
        </resolution>
        <resolution>
            <width>2560</width>
            <height>1440</height>
            <refresh_rate>60</refresh_rate>
        </resolution>
    </resolutions>
</vdd_settings>"#;
        let out = ensure_vdd_xml_high_refresh(xml, 1920, 1200);
        assert!(out.contains("<height>1200</height>"));
        assert!(vdd_xml_has_resolution(&out, 1920, 1200));
        assert!(vdd_xml_has_resolution(&out, 1920, 1080));
    }

    #[test]
    fn virtual_panel_quality_scale_matches_encode() {
        let (w, h) = virtual_panel_size(2560, 1600, 3840, 2160, 0.75, 3840, 2160, 16);
        assert_eq!((w, h), (1920, 1200));
    }

    #[test]
    fn virtual_panel_already_aligned_stays_identity() {
        let (w, h) = virtual_panel_size(1920, 1200, 3840, 2160, 1.0, 3840, 2160, 16);
        assert_eq!((w, h), (1920, 1200));
    }

    #[test]
    fn aligned_virtual_encode_does_not_need_filter_scale() {
        let tablet = (2340u32, 1080u32);
        let (enc_w, enc_h) =
            virtual_panel_size(tablet.0, tablet.1, 3840, 2160, 1.0, 3840, 2160, 16);
        let (w, h) = compute_encode_size(
            enc_w, enc_h, tablet.0, tablet.1, 3840, 2160, 1.0, 3840, 2160,
        );
        let (w, h) = (align_dim(w, 16), align_dim(h, 16));
        assert_eq!((w, h), (enc_w, enc_h));
        assert!(!crate::capture_graph::needs_scale(enc_w, enc_h, w, h));
    }

    #[test]
    fn virtual_capture_prefers_gdigrab() {
        assert!(!prefer_gdigrab_capture(true, true));
        assert!(prefer_gdigrab_capture(true, false));
        assert!(prefer_gdigrab_capture(false, false));
        assert!(!prefer_gdigrab_capture(false, true));
    }

    #[test]
    fn tablet_sleep_undoes_external_topology() {
        assert_eq!(
            client_drop_desktop_action(true, PrimaryRestoreAction::Skip),
            ClientDropDesktopAction::UndoExternal
        );
        assert_eq!(
            client_drop_desktop_action(true, PrimaryRestoreAction::SetPrimary),
            ClientDropDesktopAction::UndoExternal
        );
        assert_eq!(
            client_drop_desktop_action(false, PrimaryRestoreAction::Skip),
            ClientDropDesktopAction::None
        );
        assert_eq!(
            client_drop_desktop_action(false, PrimaryRestoreAction::TimingOnly),
            ClientDropDesktopAction::ReassertPrimary
        );
        assert_eq!(
            client_drop_desktop_action(false, PrimaryRestoreAction::SetPrimary),
            ClientDropDesktopAction::ReassertPrimary
        );
    }

    #[test]
    fn host_sleep_restores_only_tablet_only_topology() {
        assert!(is_host_suspend_power_event(4));
        assert!(!is_host_suspend_power_event(7));
        assert!(!is_host_suspend_power_event(18));
        assert!(!is_host_suspend_power_event(6));
        assert_eq!(
            host_sleep_desktop_action(true),
            ClientDropDesktopAction::UndoExternal
        );
        assert_eq!(
            host_sleep_desktop_action(false),
            ClientDropDesktopAction::None
        );
        assert!(apply_tablet_only_on_hello(false));
        assert!(!apply_tablet_only_on_hello(true));
    }

    #[test]
    fn virtual_refresh_is_120_like_glidex() {
        assert_eq!(virtual_target_hz(30, 60), 60);
        assert_eq!(virtual_target_hz(60, 60), 60);
        assert_eq!(virtual_target_hz(45, 30), 60);
        assert_eq!(virtual_target_hz(120, 90), 120);
        assert_eq!(virtual_target_hz(240, 144), 120);
        assert_eq!(encoder_start_attempts(), 1);
        assert_eq!(encoder_start_attempts_after_mode_change(), 2);
        assert!(dda_settle_after_mode_change_ms() >= 200);
        assert!(!resize_virtual_display_after_hello());
        assert!(!mirror_changes_pc_mode());
    }

    #[test]
    fn extend_one_pointer_stable_audio_and_fill_tablet() {
        assert!(!paint_cursor_overlay());
        assert!(draw_mouse_in_video(true));
        assert!(!hide_os_cursor_on_captured_display());
        assert!(suppress_mouse_trails_while_sharing());
        assert!(!mux_cursor_on_video(true, true));
        assert!(audio_packets_per_video_frame() >= 2);
        assert!(audio_flush_on_video_timeout());
        assert!(!resize_virtual_display_after_hello());
        assert!(!mirror_changes_pc_mode());
        let (w, h) = virtual_panel_size(2000, 1200, 3840, 2160, 1.0, 3840, 2160, 16);
        assert_eq!((w, h), (2000, 1200));
    }

    #[test]
    fn never_retarget_the_primary_panel() {
        assert!(!is_safe_virtual_target(r"\\.\DISPLAY1", r"\\.\DISPLAY1"));
        assert!(is_safe_virtual_target(r"\\.\DISPLAY2", r"\\.\DISPLAY1"));
        assert!(!is_safe_virtual_target("", r"\\.\DISPLAY1"));
    }

    #[test]
    fn primary_restore_skips_when_unchanged() {
        assert_eq!(
            primary_restore_action(
                Some(r"\\.\DISPLAY1"),
                true,
                2560,
                1440,
                165,
                r"\\.\DISPLAY1",
                2560,
                1440,
                165
            ),
            PrimaryRestoreAction::Skip
        );
    }

    #[test]
    fn primary_restore_timing_only_when_hz_drifted() {
        assert_eq!(
            primary_restore_action(
                Some(r"\\.\DISPLAY1"),
                true,
                2560,
                1440,
                60,
                r"\\.\DISPLAY1",
                2560,
                1440,
                165
            ),
            PrimaryRestoreAction::TimingOnly
        );
    }

    #[test]
    fn primary_restore_set_primary_when_vdd_stole_it() {
        assert_eq!(
            primary_restore_action(
                Some(r"\\.\DISPLAY1"),
                false,
                2560,
                1440,
                165,
                r"\\.\DISPLAY1",
                2560,
                1440,
                165
            ),
            PrimaryRestoreAction::SetPrimary
        );
        assert_eq!(
            primary_restore_action(None, false, 0, 0, 0, r"\\.\DISPLAY1", 2560, 1440, 165),
            PrimaryRestoreAction::SetPrimary
        );
    }

    #[test]
    fn parse_lid_indices_english_powercfg() {
        let text = r#"
Power Setting GUID: 5ca83367-6e45-459f-a27b-476b1ee90026  (Lid close action)
  GUID Alias: LIDACTION
  Minimum Possible Setting: 0x00000000
  Maximum Possible Setting: 0x00000003
Possible settings:
  0x00000000    Do nothing
  0x00000001    Sleep
Current AC Power Setting Index: 0x00000001
Current DC Power Setting Index: 0x00000002

Power Setting GUID: 7648efa3-dd9c-4e3e-b566-50f929386280  (Power button action)
  GUID Alias: PBUTTONACTION
Current AC Power Setting Index: 0x00000003
"#;
        assert_eq!(parse_lid_current_indices(text), Some((1, 2)));
    }

    #[test]
    fn parse_lid_indices_chinese_powercfg() {
        let text = r#"
电源设置 GUID: 5ca83367-6e45-459f-a27b-476b1ee90026  (合上盖子时的操作)
  GUID Alias: LIDACTION
当前交流电源设置索引: 0x00000000
当前直流电源设置索引: 0x00000001
电源设置 GUID: 7648efa3-dd9c-4e3e-b566-50f929386280
"#;
        assert_eq!(parse_lid_current_indices(text), Some((0, 1)));
    }

    #[test]
    fn usb2_holds_game_bitrate() {
        assert!(usb2_can_carry_bitrate_kbps(25_000));
        assert!(usb2_can_carry_bitrate_kbps(40_000));
        assert!(!usb2_can_carry_bitrate_kbps(120_000));
    }

    #[test]
    fn encoded_backpressure_does_not_tear_gop() {
        assert!(!drop_encoded_p_on_backpressure());
        assert_eq!(encoded_queue_capacity(), 0);
        assert_eq!(annexb_raw_queue_capacity(), 0);
        assert_eq!(capture_thread_queue_size(), 1);
        assert!(!coalesce_extra_video_on_write());
    }

    #[test]
    fn encoded_queue_rendezvous_does_not_park_an_au() {
        assert_eq!(encoded_queue_capacity(), 0);
        let (tx, rx) = std::sync::mpsc::sync_channel::<u8>(encoded_queue_capacity());
        let done = std::thread::spawn(move || {
            tx.send(1).unwrap();
            tx.send(2).unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(
            !done.is_finished(),
            "rendezvous must block before recv; a cap-1 queue would park 1"
        );
        assert_eq!(rx.recv().unwrap(), 1);
        assert_eq!(rx.recv().unwrap(), 2);
        done.join().unwrap();
    }

    #[test]
    fn encoder_follows_capture_gpu() {
        assert_eq!(encoder_fallback_chain("avc", 0x8086)[0], "h264_qsv");
        assert_eq!(encoder_fallback_chain("avc", 0x10DE)[0], "h264_nvenc");
        assert_eq!(encoder_fallback_chain("avc", 0x1002)[0], "h264_amf");
        assert_eq!(encoder_fallback_chain("hevc", 0x8086)[0], "hevc_qsv");
    }

    #[test]
    fn skips_cuda_family_when_derive_is_unimplemented() {
        let err = "Device creation failed: -40.\nFailed to set value 'cuda=cuda@capture' for option 'init_hw_device': Function not implemented\nError parsing global options: Function not implemented";
        assert_eq!(skip_hw_family_after_stderr(err), Some("cuda"));
        let cuda = "ddagrab=output_idx=1:framerate=60,hwmap=derive_device=cuda:reverse=1:extra_hw_frames=0,scale_cuda=1800:1080:format=nv12";
        let cpu = "ddagrab=output_idx=1:framerate=60,hwdownload,format=bgra,scale=1800:1080:flags=bilinear,format=yuv420p";
        assert!(graph_uses_hw_family(cuda, "cuda"));
        assert!(!graph_uses_hw_family(cpu, "cuda"));
        assert!(!graph_uses_hw_family(cpu, "qsv"));
        assert!(!graph_uses_hw_family(cpu, "amf"));
        assert_eq!(
            skip_hw_family_after_stderr(
                "The filters 'Parsed_format_2' and 'Parsed_format_3' do not have a common format"
            ),
            None
        );
    }

    #[test]
    fn skips_qsv_and_amf_when_device_init_is_dead() {
        assert_eq!(
            skip_hw_family_after_stderr(
                "[QSV] Error creating a MFX session: -9.\nFailed to set value 'qsv=qsv@capture'"
            ),
            Some("qsv")
        );
        assert_eq!(
            skip_hw_family_after_stderr(
                "AMF failed to initialise on the given D3D11 device: 4.\nFailed to set value 'amf=amf@capture' for option 'init_hw_device': No such device"
            ),
            Some("amf")
        );
        assert!(graph_uses_hw_family(
            "ddagrab,hwmap=derive_device=qsv:reverse=1:extra_hw_frames=0,scale_qsv=w=1800:h=1080:format=nv12",
            "qsv"
        ));
        assert!(graph_uses_hw_family(
            "ddagrab,hwmap=derive_device=amf:reverse=1:extra_hw_frames=0,vpp_amf=w=1800:h=1080:format=nv12",
            "amf"
        ));
    }

    #[test]
    fn vdd_prefers_nvidia_name() {
        let adapters = [
            (0x8086, "Intel(R) UHD Graphics".into()),
            (0x10DE, "NVIDIA GeForce RTX 4060 Laptop GPU".into()),
        ];
        assert!(pick_vdd_gpu_name(&adapters).contains("NVIDIA"));
        assert_eq!(pick_vdd_gpu_name(&[]), "Best GPU (Auto)");
    }
}
