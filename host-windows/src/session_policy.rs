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

/// GlideX / SuperDisplay run the virtual panel at 120 Hz even when the tablet
/// is 60 Hz. A 60 Hz IddCx mode makes DWM/DDA wait a 16 ms vsync; 120 Hz
/// cuts that wait in half. Never above 120: IddCx mode tables get sparse, and
/// higher values used to retime the laptop panel.
pub fn virtual_target_hz(_requested: u32, _tablet_max: u32) -> u32 {
    120
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

pub fn jitter_backoff_ms(base_ms: u64, jitter_ms: u64, max_jitter_ms: u64) -> u64 {
    base_ms.saturating_add(jitter_ms.min(max_jitter_ms))
}

/// USB 2.0 Hi-Speed practically carries ~25–35 MB/s. 25 Mbps video is ~3 MB/s,
/// so "not even 30 Hz" is not a USB bandwidth ceiling.
pub fn usb2_can_carry_bitrate_kbps(bitrate_kbps: u32) -> bool {
    bitrate_kbps <= 80_000
}

/// Encoded AU queue. Deep enough to absorb an IDR spike without tearing a GOP.
pub fn encoded_queue_capacity() -> usize {
    1
}

/// Raw annexb reader→assembler queue. 8 held ~8 AUs (60–130 ms at 120 Hz)
/// so ffmpeg never saw backpressure and `encoded_queue_capacity(1)` could
/// not make it skip *input* frames. 2 fits one Data+Quiet burst.
pub fn annexb_raw_queue_capacity() -> usize {
    2
}

/// ffmpeg `-thread_queue_size`. One queued capture is ~16 ms at 60 Hz;
/// two was still a visible glass delay next to a real monitor.
pub fn capture_thread_queue_size() -> u32 {
    1
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

/// ffmpeg hwupload/hwmap pool. Default is often 16 frames (~250 ms).
pub fn hw_extra_frames() -> u32 {
    1
}

/// Second CUDA/QSV pool size if a 1-frame pool stalls ffmpeg.
pub fn hw_extra_frames_fallback() -> u32 {
    2
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

/// Encoder DPB / `num_ref_frames`. NVENC default follows the level (4–16);
/// Android then holds decoded pictures. Moonlight decoder-errata: 1.
pub fn encoder_refs() -> u32 {
    1
}

/// libx265 still look-aheads unless these are explicit. `-tune zerolatency`
/// is not enough on some builds (rc-lookahead stays ~20 → a GOP of glass wait).
pub fn x265_params(gop: u32) -> String {
    let keyint = gop.max(1);
    format!(
        "bframes=0:ref=1:no-open-gop:keyint={keyint}:min-keyint={keyint}:rc-lookahead=0:sync-lookahead=0:frame-threads=1:no-scenecut:repeat-headers=1"
    )
}

/// WASAPI shared-mode loopback buffer, 100-ns units. 50 ms was audible lag.
pub fn wasapi_buffer_hns() -> i64 {
    200_000
}

/// Host audio packets waiting for the next video AU. take_latest keeps one.
pub fn audio_capture_queue() -> usize {
    2
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

/// TCP send buffer. One encoded P-frame at 25 Mbps / 120 fps (~26 KB).
/// 48 KB was sized for 60 fps and sat ~15 ms on USB adb reverse after
/// annexb/encoded queues were already 1–2 deep.
pub fn tcp_send_buffer_bytes() -> usize {
    24 * 1024
}

/// TCP recv buffer on the video socket (host side, mostly unused).
pub fn tcp_recv_buffer_bytes() -> usize {
    48 * 1024
}

/// Control-plane socket: cursor packets are tens of bytes.
pub fn tcp_control_buffer_bytes() -> usize {
    16 * 1024
}

/// ffmpeg `ddagrab` `dup_frames`. Duplicating up to the timer adds a wait that
/// a real monitor does not have. Unique desktop frames only.
pub fn ddagrab_duplicate_frames() -> bool {
    false
}

/// ffmpeg `ddagrab` sleeps to 1/framerate *before* AcquireNextFrame
/// (`vsrc_ddagrab.c`). 120 Hz parked a ready DXGI frame on an 8 ms grid;
/// 1000 Hz still sat every unique frame on a 1 ms tick. 8000 Hz ≈ 125 µs.
/// `dup_frames=0` so this is poll rate, not encode rate. Not for gdigrab.
pub fn dda_poll_hz(_encode_fps: u32) -> u32 {
    8000
}

/// Audio must never drain ahead of a video AU on the same TCP writer.
/// Keep the latest packet only so loopback cannot HOL-block the desktop.
pub fn audio_packets_per_video_frame() -> usize {
    1
}

/// GlideX / SuperDisplay encode at the virtual panel (120 Hz), not the
/// tablet vsync. `min(decoder_max_fps)` used to pin a 60 Hz pad to a 16 ms
/// encode grid even after IddCx was already presenting every 8 ms.
pub fn encode_fps(_req_fps: u32, _tablet_max: u32, _dec_fps: u32, hw: bool) -> u32 {
    if !hw {
        45
    } else {
        120
    }
}

/// Anonymous `CreatePipe` default is 4 KB. A 25 Mbps P-frame is ~26 KB, so
/// ffmpeg stdout used to land as many short `Read`s and PeekNamedPipe went
/// quiet between them. 256 KB still hid ~10 pictures at 120 Hz after the
/// encoded/annexb queues were already 1–2 deep. 64 KB fits one IDR-sized
/// AU without going back to 4 KB fragments.
pub fn ffmpeg_pipe_buffer_bytes() -> u32 {
    64 * 1024
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
        block.push_str(&format!("            <refresh_rate>{hz}</refresh_rate>
"));
    }
    block.push_str("        </resolution>
");
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
    let w_tag = format!("<width>{width}</width>");
    let h_tag = format!("<height>{height}</height>");
    if !(xml.contains(&w_tag) && xml.contains(&h_tag)) {
        let entry = vdd_resolution_xml(width, height);
        if let Some(idx) = xml.find("</resolutions>") {
            xml.insert_str(idx, &entry);
        }
    }
    xml
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
        native_panel_mode(modes).map(|(w, h, _)| (w, h)).unwrap_or((target_w, target_h))
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

fn pick_in_pool(
    pool: &[(u32, u32, u32)],
    target_w: u32,
    target_h: u32,
) -> Option<(u32, u32, u32)> {
    let mut best: Option<((u128, u32), (u32, u32, u32))> = None;
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
        assert_eq!(cursor_sample_interval_ms(), 1);
        assert_eq!(hw_extra_frames(), 1);
        assert_eq!(hw_extra_frames_fallback(), 2);
        assert_eq!(nvenc_surfaces(), 1);
        assert_eq!(nvenc_surfaces_fallback(), 2);
        assert_eq!(nvenc_rc(), "cbr_ld_hq");
        assert_ne!(nvenc_rc(), "cbr");
        assert_eq!(nvenc_rc_attempts(), vec!["cbr_ld_hq", "cbr"]);
        assert_eq!(amf_async_depth(), 1);
        assert_eq!(encoder_refs(), 1);
        assert!(x265_params(120).contains("rc-lookahead=0"));
        assert!(x265_params(120).contains("bframes=0"));
        assert!(x265_params(120).contains("keyint=120"));
        assert_eq!(nvenc_surface_attempts(), vec![1, 2]);
        assert!(!nvenc_spatial_aq());
        assert_eq!(wasapi_buffer_hns(), 200_000);
        assert_eq!(audio_capture_queue(), 2);
        assert!(!ddagrab_duplicate_frames());
        assert_eq!(dda_poll_hz(60), 8000);
        assert_eq!(dda_poll_hz(120), 8000);
        assert_eq!(dda_poll_hz(144), 8000);
        assert_eq!(dda_poll_hz(30), 8000);
        assert_eq!(tcp_send_buffer_bytes(), 24 * 1024);
        assert!(tcp_control_buffer_bytes() < tcp_send_buffer_bytes());
    }

    #[test]
    fn encode_fps_matches_virtual_120_like_glidex() {
        assert_eq!(encode_fps(60, 120, 120, true), 120);
        assert_eq!(encode_fps(60, 90, 60, true), 120);
        assert_eq!(encode_fps(60, 120, 60, false), 45);
        assert_eq!(encode_fps(30, 60, 60, true), 120);
        assert_eq!(ffmpeg_pipe_buffer_bytes(), 64 * 1024);
        assert!(ffmpeg_pipe_buffer_bytes() > 16 * 1024);
        assert_eq!(audio_packets_per_video_frame(), 1);
        assert_eq!(control_attach_wait_ms(), 0);
        assert!(mux_cursor_on_video(true, false));
        assert!(!mux_cursor_on_video(true, true));
        assert!(!mux_cursor_on_video(false, false));
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
    fn virtual_refresh_is_120_like_glidex() {
        assert_eq!(virtual_target_hz(30, 60), 120);
        assert_eq!(virtual_target_hz(60, 60), 120);
        assert_eq!(virtual_target_hz(45, 30), 120);
        assert_eq!(virtual_target_hz(120, 90), 120);
        assert_eq!(virtual_target_hz(240, 144), 120);
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
        assert_eq!(encoded_queue_capacity(), 1);
        assert_eq!(annexb_raw_queue_capacity(), 2);
        assert_eq!(capture_thread_queue_size(), 1);
    }

    #[test]
    fn encoder_follows_capture_gpu() {
        assert_eq!(encoder_fallback_chain("avc", 0x8086)[0], "h264_qsv");
        assert_eq!(encoder_fallback_chain("avc", 0x10DE)[0], "h264_nvenc");
        assert_eq!(encoder_fallback_chain("avc", 0x1002)[0], "h264_amf");
        assert_eq!(encoder_fallback_chain("hevc", 0x8086)[0], "hevc_qsv");
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
