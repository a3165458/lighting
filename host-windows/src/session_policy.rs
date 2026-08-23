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
    fn classifies_common_disconnects() {
        assert!(is_client_disconnect("send video failed: broken pipe"));
        assert!(is_client_disconnect("connection reset by peer"));
        assert!(is_client_disconnect("early eof"));
        assert!(is_client_disconnect("os error 10054"));
        assert!(!is_client_disconnect("所选显示器不存在"));
        assert!(!is_client_disconnect("找不到 ffmpeg"));
    }
}
