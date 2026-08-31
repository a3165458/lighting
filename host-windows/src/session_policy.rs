/// After a client socket ends, keep the share session on the same listen
/// socket (and adb reverse) unless the user explicitly stopped sharing.
pub fn continue_accept_loop(user_stopped: bool) -> bool {
    !user_stopped
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

/// Smooth heartbeat round-trips so the latency tile does not flicker on a single
/// slow reply. `prev` 0 means "no sample yet", so the first reading is taken raw.
pub fn smooth_latency_ms(prev: u32, sample: u32) -> u32 {
    if prev == 0 {
        sample
    } else {
        (prev.saturating_mul(3) + sample) / 4
    }
}

/// ~4-frame VBV: low latency without the quality swings of a 1–2 frame budget.
pub fn vbv_bufsize_kb(bitrate_kbps: u32, fps: u32) -> u32 {
    let fps = fps.max(24);
    ((bitrate_kbps * 4) / fps).clamp(1_200, bitrate_kbps.max(1_200))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disconnect_keeps_listening() {
        assert!(continue_accept_loop(false));
        assert!(!continue_accept_loop(true));
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
    fn vbv_targets_about_four_frames() {
        // 25 Mbps @ 60fps → ~1666 kb for 4 frames
        assert_eq!(vbv_bufsize_kb(25_000, 60), 1_666);
        assert!(vbv_bufsize_kb(8_000, 120) >= 1_200);
        assert!(vbv_bufsize_kb(40_000, 30) <= 40_000);
        // Old formula used bitrate/2; keep the new budget far below that.
        assert!(vbv_bufsize_kb(25_000, 60) < 25_000 / 2);
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
}
