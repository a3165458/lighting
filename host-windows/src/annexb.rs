use anyhow::{bail, Context, Result};
use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

/// Pump output. Tests use a std channel; the live encoder uses tokio so the
/// send loop can `recv().await` instead of polling every 1 ms.
pub trait PacketSink: Send {
    fn try_push(&self, pkt: EncodedPacket) -> TryPush;
    fn push_blocking(&self, pkt: EncodedPacket);
}

#[derive(Debug)]
pub enum TryPush {
    Sent,
    Full(EncodedPacket),
    Closed,
}

impl PacketSink for mpsc::SyncSender<EncodedPacket> {
    fn try_push(&self, pkt: EncodedPacket) -> TryPush {
        match self.try_send(pkt) {
            Ok(()) => TryPush::Sent,
            Err(mpsc::TrySendError::Full(p)) => TryPush::Full(p),
            Err(mpsc::TrySendError::Disconnected(_)) => TryPush::Closed,
        }
    }
    fn push_blocking(&self, pkt: EncodedPacket) {
        let _ = self.send(pkt);
    }
}

impl PacketSink for tokio::sync::mpsc::Sender<EncodedPacket> {
    fn try_push(&self, pkt: EncodedPacket) -> TryPush {
        match self.try_send(pkt) {
            Ok(()) => TryPush::Sent,
            Err(tokio::sync::mpsc::error::TrySendError::Full(p)) => TryPush::Full(p),
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => TryPush::Closed,
        }
    }
    fn push_blocking(&self, pkt: EncodedPacket) {
        let _ = self.blocking_send(pkt);
    }
}

/// After ffmpeg `-flush_packets 1` the pipe goes quiet until the next picture.
/// Waiting for that next start code is one refresh of glass delay. Waiting 0
/// on a short `Read` is worse: Windows pipes return partial AUs. Idle 1 ms
/// (process `timeBeginPeriod(1)`) is the portable fallback. Live capture uses
/// PeekNamedPipe + a 250 µs spin so we do not sit on a 1 ms timer tick.
const IDLE_FLUSH: Duration = Duration::from_millis(1);
const PIPE_QUIET: Duration = Duration::from_micros(250);

fn read_buffer_bytes() -> usize {
    crate::session_policy::ffmpeg_pipe_buffer_bytes().max(1) as usize
}

enum RawMsg {
    Data(Vec<u8>),
    Quiet,
}

fn raise_reader_priority() {
    // ffmpeg is HIGH_PRIORITY_CLASS. The assembler thread is already
    // THREAD_PRIORITY_HIGHEST; this reader was NORMAL and could sit a
    // scheduler slice behind egui while a 32 KB pipe AU waited.
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
        };
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    }
    enter_mmcss();
}

/// Register this thread with MMCSS so a foreground game cannot park a
/// ready AU on a 15.6 ms scheduler tick. Handle is leaked on purpose:
/// AvRevertMmThreadCharacteristics would drop us off the 1 ms boost.
pub fn enter_mmcss() {
    #[cfg(windows)]
    enter_mmcss_windows();
}

#[cfg(windows)]
fn enter_mmcss_windows() {
    if !crate::session_policy::mmcss_capture_threads() {
        return;
    }
    unsafe {
        use windows::Win32::System::Threading::{
            AvSetMmThreadCharacteristicsW, AvSetMmThreadPriority, AVRT_PRIORITY_HIGH,
        };
        let mut task_index = 0u32;
        let handle = AvSetMmThreadCharacteristicsW(windows::core::w!("Games"), &mut task_index)
            .or_else(|_| {
                AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &mut task_index)
            });
        if let Ok(handle) = handle {
            let _ = AvSetMmThreadPriority(handle, AVRT_PRIORITY_HIGH);
        }
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPacket {
    pub data: Vec<u8>,
    pub keyframe: bool,
    pub codec_config: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NalRole {
    ParameterSet,
    Aud,
    Idr,
    Vcl,
    /// Prefix/suffix SEI. NVENC pic_timing / buffering_period makes Android
    /// hold the picture for the CPB delay even after VUI timing is stripped.
    Sei,
    Other,
}

/// Collect codec-config + IDR from a freshly started encoder before P-frames
/// are safe to send to a live Android decoder.
#[derive(Debug, Default)]
pub struct BootstrapCollector {
    packets: Vec<EncodedPacket>,
    have_cfg: bool,
    have_key: bool,
}

impl BootstrapCollector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn complete(&self) -> bool {
        self.have_cfg && self.have_key
    }

    pub fn packets(&self) -> &[EncodedPacket] {
        &self.packets
    }

    pub fn into_packets(self) -> Vec<EncodedPacket> {
        self.packets
    }

    /// Push one encoder packet. P-frames are dropped until bootstrap is ready.
    /// Returns true once both codec-config and a keyframe have been collected.
    pub fn push(&mut self, pkt: EncodedPacket, hevc: bool) -> bool {
        if self.complete() {
            return true;
        }
        let is_cfg = pkt.codec_config || looks_like_codec_config(&pkt.data, hevc);
        let is_key = pkt.keyframe || looks_like_idr(&pkt.data, hevc);
        if is_cfg && !is_key {
            self.have_cfg = true;
            self.packets.push(EncodedPacket {
                data: pkt.data,
                keyframe: false,
                codec_config: true,
            });
            return self.complete();
        }
        if is_key {
            if !self.have_cfg {
                if let Some(cfg) = extract_parameter_sets(&pkt.data, hevc) {
                    self.packets.push(EncodedPacket {
                        data: cfg,
                        keyframe: false,
                        codec_config: true,
                    });
                    self.have_cfg = true;
                }
            }
            self.have_key = true;
            self.packets.push(EncodedPacket {
                data: pkt.data,
                keyframe: true,
                codec_config: false,
            });
        }
        self.complete()
    }
}

pub fn recv_bootstrap(
    rx: &Receiver<EncodedPacket>,
    timeout: Duration,
    hevc: bool,
) -> Result<Vec<EncodedPacket>> {
    let deadline = Instant::now() + timeout;
    let mut collector = BootstrapCollector::new();
    while !collector.complete() {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match rx.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(pkt) => {
                collector.push(pkt, hevc);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                bail!("encoder pipe closed before codec-config + IDR");
            }
        }
    }
    if collector.complete() {
        Ok(collector.into_packets())
    } else {
        bail!("encoder restart did not emit codec-config + IDR in time")
    }
}

pub async fn recv_bootstrap_async(
    rx: &mut tokio::sync::mpsc::Receiver<EncodedPacket>,
    timeout: Duration,
    hevc: bool,
) -> Result<Vec<EncodedPacket>> {
    let deadline = Instant::now() + timeout;
    let mut collector = BootstrapCollector::new();
    while !collector.complete() {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match tokio::time::timeout(deadline.saturating_duration_since(now), rx.recv()).await {
            Ok(Some(pkt)) => {
                collector.push(pkt, hevc);
            }
            Ok(None) => bail!("encoder pipe closed before codec-config + IDR"),
            Err(_) => break,
        }
    }
    if collector.complete() {
        Ok(collector.into_packets())
    } else {
        bail!("encoder restart did not emit codec-config + IDR in time")
    }
}

pub fn looks_like_codec_config(data: &[u8], hevc: bool) -> bool {
    split_annexb_complete(data)
        .into_iter()
        .any(|nal| nal_role(&nal, hevc) == NalRole::ParameterSet)
}

pub fn looks_like_idr(data: &[u8], hevc: bool) -> bool {
    split_annexb_complete(data)
        .into_iter()
        .any(|nal| nal_role(&nal, hevc) == NalRole::Idr)
}

pub fn extract_parameter_sets(data: &[u8], hevc: bool) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for nal in split_annexb_complete(data) {
        if nal_role(&nal, hevc) == NalRole::ParameterSet {
            out.extend_from_slice(&nal);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn start_code_len(nal: &[u8]) -> usize {
    if nal.len() >= 4 && nal[0] == 0 && nal[1] == 0 && nal[2] == 0 && nal[3] == 1 {
        4
    } else if nal.len() >= 3 && nal[0] == 0 && nal[1] == 0 && nal[2] == 1 {
        3
    } else {
        0
    }
}

pub fn h264_nal_type(nal: &[u8]) -> u8 {
    let i = start_code_len(nal);
    nal.get(i).map(|b| b & 0x1F).unwrap_or(0)
}

pub fn hevc_nal_type(nal: &[u8]) -> u8 {
    let i = start_code_len(nal);
    nal.get(i).map(|b| (b >> 1) & 0x3F).unwrap_or(0)
}

pub fn nal_role(nal: &[u8], hevc: bool) -> NalRole {
    if hevc {
        match hevc_nal_type(nal) {
            32 | 33 | 34 => NalRole::ParameterSet,
            35 => NalRole::Aud,
            39 | 40 => NalRole::Sei,
            19 | 20 | 21 => NalRole::Idr,
            0..=31 => NalRole::Vcl,
            _ => NalRole::Other,
        }
    } else {
        match h264_nal_type(nal) {
            6 => NalRole::Sei,
            7 | 8 => NalRole::ParameterSet,
            9 => NalRole::Aud,
            5 => NalRole::Idr,
            1..=4 => NalRole::Vcl,
            _ => NalRole::Other,
        }
    }
}

fn starts_parameter_set_group(nal: &[u8], hevc: bool) -> bool {
    if hevc {
        hevc_nal_type(nal) == 32
    } else {
        h264_nal_type(nal) == 7
    }
}

pub fn pump_annexb(
    stdout: impl Read + Send + 'static,
    tx: impl PacketSink + 'static,
    hevc: bool,
) -> Result<()> {
    pump_annexb_with_available(stdout, tx, hevc, |_| None)
}

/// `available` is PeekNamedPipe on the live ffmpeg stdout. `None` means
/// "unknown" (tests / non-pipe) and we fall back to `idle`.
pub fn pump_annexb_with_available<R, F>(
    stdout: R,
    tx: impl PacketSink + 'static,
    hevc: bool,
    available: F,
) -> Result<()>
where
    R: Read + Send + 'static,
    F: Fn(&R) -> Option<usize> + Send + 'static,
{
    pump_annexb_with_available_idle(stdout, tx, hevc, available, IDLE_FLUSH)
}

fn pump_annexb_with_available_idle<R, F>(
    mut stdout: R,
    tx: impl PacketSink + 'static,
    hevc: bool,
    available: F,
    idle: Duration,
) -> Result<()>
where
    R: Read + Send + 'static,
    F: Fn(&R) -> Option<usize> + Send + 'static,
{
    let (raw_tx, raw_rx) = mpsc::sync_channel::<RawMsg>(
        crate::session_policy::annexb_raw_queue_capacity().max(1),
    );
    let reader = thread::Builder::new()
        .name("lighting-annexb-read".into())
        .spawn(move || {
            raise_reader_priority();
            // Same size as CreatePipe. A vec larger than the pipe never
            // fills, so n == buf.len() is dead and a full-pipe IDR looks
            // like a short AU: Quiet truncated the slice and the tablet
            // waited until the next keyframe.
            let mut buf = vec![0u8; read_buffer_bytes()];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if raw_tx.send(RawMsg::Data(buf[..n].to_vec())).is_err() {
                            break;
                        }
                        let Some(mut avail) = available(&stdout) else {
                            continue;
                        };
                        // A short read + empty pipe is the end of this AU.
                        // Spinning 250 µs here sat every 120 Hz picture on a
                        // timer that GlideX does not pay. Only wait if the
                        // buffer was full (the AU may still be arriving).
                        if n == buf.len() {
                            let spin = Instant::now();
                            while avail == 0 && spin.elapsed() < PIPE_QUIET {
                                std::hint::spin_loop();
                                avail = available(&stdout).unwrap_or(0);
                            }
                        }
                        if avail == 0 {
                            let _ = raw_tx.send(RawMsg::Quiet);
                        }
                    }
                }
            }
        })
        .context("annexb reader thread")?;

    let mut acc = Vec::with_capacity(256 * 1024);
    let mut au = Vec::new();
    let mut au_has_vcl = false;
    let mut au_key = false;
    let mut sps_pps = Vec::new();
    let mut sent_cfg = false;
    let mut drop_until_key = false;

    loop {
        match raw_rx.recv_timeout(idle) {
            Ok(RawMsg::Data(chunk)) => {
                acc.extend_from_slice(&chunk);
                for nal in split_annexb(&mut acc) {
                    ingest_nal(
                        nal,
                        hevc,
                        &mut au,
                        &mut au_has_vcl,
                        &mut au_key,
                        &mut sps_pps,
                        &mut sent_cfg,
                        &tx,
                        &mut drop_until_key,
                    );
                }
            }
            Ok(RawMsg::Quiet) | Err(mpsc::RecvTimeoutError::Timeout) => {
                finish_flushed_au(
                    &mut acc,
                    hevc,
                    &mut au,
                    &mut au_has_vcl,
                    &mut au_key,
                    &mut sps_pps,
                    &mut sent_cfg,
                    &tx,
                    &mut drop_until_key,
                );
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if start_code_len(&acc) > 0 {
                    ingest_nal(
                        std::mem::take(&mut acc),
                        hevc,
                        &mut au,
                        &mut au_has_vcl,
                        &mut au_key,
                        &mut sps_pps,
                        &mut sent_cfg,
                        &tx,
                        &mut drop_until_key,
                    );
                }
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
        }
    }
    let _ = reader.join();
    Ok(())
}

fn finish_flushed_au(
    acc: &mut Vec<u8>,
    hevc: bool,
    au: &mut Vec<u8>,
    au_has_vcl: &mut bool,
    au_key: &mut bool,
    sps_pps: &mut Vec<u8>,
    sent_cfg: &mut bool,
    tx: &dyn PacketSink,
    drop_until_key: &mut bool,
) {
    if start_code_len(acc) > 0 {
        ingest_nal(
            std::mem::take(acc),
            hevc,
            au,
            au_has_vcl,
            au_key,
            sps_pps,
            sent_cfg,
            tx,
            drop_until_key,
        );
    }
    if *au_has_vcl {
        flush_au(au, au_has_vcl, au_key, sps_pps, tx, drop_until_key);
    }
}

fn ingest_nal(
    nal: Vec<u8>,
    hevc: bool,
    au: &mut Vec<u8>,
    au_has_vcl: &mut bool,
    au_key: &mut bool,
    sps_pps: &mut Vec<u8>,
    sent_cfg: &mut bool,
    tx: &dyn PacketSink,
    drop_until_key: &mut bool,
) {
    let nal = if hevc {
        crate::hevc_sps::rewrite_low_latency(nal)
    } else {
        crate::h264_sps::rewrite_low_latency(nal)
    };
    let role = nal_role(&nal, hevc);
    if role == NalRole::Sei {
        return;
    }
    if role == NalRole::ParameterSet {
        if starts_parameter_set_group(&nal, hevc) {
            sps_pps.clear();
        }
        sps_pps.extend_from_slice(&nal);
        return;
    }
    if !*sent_cfg && !sps_pps.is_empty() {
        tx.push_blocking(EncodedPacket {
            data: sps_pps.clone(),
            keyframe: false,
            codec_config: true,
        });
        *sent_cfg = true;
    }
    let is_vcl = matches!(role, NalRole::Idr | NalRole::Vcl);
    if role == NalRole::Aud || (is_vcl && *au_has_vcl) {
        flush_au(au, au_has_vcl, au_key, sps_pps, tx, drop_until_key);
    }
    if role == NalRole::Idr {
        *au_key = true;
    }
    if is_vcl {
        *au_has_vcl = true;
    }
    au.extend_from_slice(&nal);
}

fn flush_au(
    au: &mut Vec<u8>,
    has_vcl: &mut bool,
    key: &mut bool,
    sps_pps: &[u8],
    tx: &dyn PacketSink,
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
    match tx.try_push(pkt) {
        TryPush::Sent => {
            *drop_until_key = false;
        }
        TryPush::Full(pkt) => {
            // Block rather than drop a P-frame. Skipping encoded P-frames
            // forces the tablet to wait for the next IDR (~1s) and looks
            // like "not even 30 Hz" on USB jitter, which is not a bandwidth
            // limit. ffmpeg then skips *input* frames instead of tearing the GOP.
            if crate::session_policy::drop_encoded_p_on_backpressure() && !pkt.keyframe {
                *drop_until_key = true;
            } else {
                tx.push_blocking(pkt);
                *drop_until_key = false;
            }
        }
        TryPush::Closed => {}
    }
    *has_vcl = false;
    *key = false;
}

/// Split complete NALs from `acc`, leaving a trailing incomplete fragment.
pub fn split_annexb(acc: &mut Vec<u8>) -> Vec<Vec<u8>> {
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

/// Split a complete Annex-B buffer, including the trailing NAL.
pub fn split_annexb_complete(data: &[u8]) -> Vec<Vec<u8>> {
    let mut acc = data.to_vec();
    // A sentinel start code lets split_annexb emit the last real NAL.
    acc.extend_from_slice(&[0, 0, 0, 1]);
    split_annexb(&mut acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn nal(header: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 1, header];
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn enter_mmcss_does_not_panic() {
        enter_mmcss();
    }

    #[test]
    fn h264_roles() {
        assert_eq!(nal_role(&nal(0x67, &[]), false), NalRole::ParameterSet);
        assert_eq!(nal_role(&nal(0x68, &[]), false), NalRole::ParameterSet);
        assert_eq!(nal_role(&nal(0x65, &[]), false), NalRole::Idr);
        assert_eq!(nal_role(&nal(0x41, &[]), false), NalRole::Vcl);
        assert_eq!(nal_role(&nal(0x09, &[]), false), NalRole::Aud);
        assert_eq!(nal_role(&nal(0x06, &[]), false), NalRole::Sei);
    }

    #[test]
    fn hevc_roles() {
        assert_eq!(nal_role(&nal(0x40, &[]), true), NalRole::ParameterSet); // VPS 32
        assert_eq!(nal_role(&nal(0x42, &[]), true), NalRole::ParameterSet); // SPS 33
        assert_eq!(nal_role(&nal(0x44, &[]), true), NalRole::ParameterSet); // PPS 34
        assert_eq!(nal_role(&nal(0x28, &[]), true), NalRole::Idr); // IDR_N_LP 20
        assert_eq!(nal_role(&nal(0x26, &[]), true), NalRole::Idr); // IDR_W_RADL 19
        assert_eq!(nal_role(&nal(0x02, &[]), true), NalRole::Vcl);
        assert_eq!(nal_role(&nal(0x4E, &[]), true), NalRole::Sei); // PREFIX_SEI 39
        assert_eq!(nal_role(&nal(0x50, &[]), true), NalRole::Sei); // SUFFIX_SEI 40
    }

    #[test]
    fn bootstrap_skips_p_frames_until_cfg_and_idr() {
        let mut c = BootstrapCollector::new();
        assert!(!c.push(
            EncodedPacket {
                data: nal(0x41, &[1]),
                keyframe: false,
                codec_config: false,
            },
            false
        ));
        assert!(!c.push(
            EncodedPacket {
                data: nal(0x67, &[2]).into_iter().chain(nal(0x68, &[3])).collect(),
                keyframe: false,
                codec_config: true,
            },
            false
        ));
        assert!(c.push(
            EncodedPacket {
                data: nal(0x65, &[4]),
                keyframe: true,
                codec_config: false,
            },
            false
        ));
        assert_eq!(c.packets().len(), 2);
        assert!(c.packets()[0].codec_config);
        assert!(c.packets()[1].keyframe);
    }

    #[test]
    fn bootstrap_synthesizes_config_from_prefixed_idr() {
        let mut c = BootstrapCollector::new();
        let mut idr = nal(0x67, &[9]);
        idr.extend_from_slice(&nal(0x68, &[8]));
        idr.extend_from_slice(&nal(0x65, &[7]));
        assert!(c.push(
            EncodedPacket {
                data: idr,
                keyframe: true,
                codec_config: false,
            },
            false
        ));
        assert_eq!(c.packets().len(), 2);
        assert!(c.packets()[0].codec_config);
        assert!(!c.packets()[0].keyframe);
        assert!(looks_like_codec_config(&c.packets()[0].data, false));
        assert!(c.packets()[1].keyframe);
    }

    #[test]
    fn bootstrap_hevc_from_vps_then_idr() {
        let mut c = BootstrapCollector::new();
        let mut cfg = nal(0x40, &[1]);
        cfg.extend_from_slice(&nal(0x42, &[2]));
        cfg.extend_from_slice(&nal(0x44, &[3]));
        assert!(!c.push(
            EncodedPacket {
                data: cfg,
                keyframe: false,
                codec_config: true,
            },
            true
        ));
        assert!(c.push(
            EncodedPacket {
                data: nal(0x28, &[4]),
                keyframe: true,
                codec_config: false,
            },
            true
        ));
        assert!(c.complete());
    }

    #[test]
    fn pump_emits_config_then_idr_for_h264() {
        let mut stream = Vec::new();
        stream.extend_from_slice(&nal(0x67, &[0x42]));
        stream.extend_from_slice(&nal(0x68, &[0xCE]));
        stream.extend_from_slice(&nal(0x65, &[0xAA]));
        stream.extend_from_slice(&nal(0x41, &[0xBB]));
        let (tx, rx) = mpsc::sync_channel(8);
        pump_annexb(Cursor::new(stream), tx, false).unwrap();
        let first = rx.recv().unwrap();
        assert!(first.codec_config);
        assert!(!first.keyframe);
        let second = rx.recv().unwrap();
        assert!(second.keyframe);
        assert!(looks_like_codec_config(&second.data, false));
    }

    #[test]
    fn drops_sei_so_decoder_does_not_pace() {
        let mut stream = Vec::new();
        stream.extend_from_slice(&nal(0x67, &[0x42]));
        stream.extend_from_slice(&nal(0x68, &[0xCE]));
        stream.extend_from_slice(&nal(0x06, &[0, 1, 2, 3]));
        stream.extend_from_slice(&nal(0x65, &[0xAA]));
        let (tx, rx) = mpsc::sync_channel(8);
        pump_annexb(Cursor::new(stream), tx, false).unwrap();
        let cfg = rx.recv().unwrap();
        assert!(cfg.codec_config);
        let idr = rx.recv().unwrap();
        assert!(idr.keyframe);
        assert!(
            !idr.data.windows(5).any(|w| w == [0, 0, 0, 1, 0x06]),
            "pic_timing SEI must not reach Android"
        );
        assert!(idr.data.windows(5).any(|w| w == [0, 0, 0, 1, 0x65]));
    }

    #[test]
    fn pump_emits_config_then_idr_for_hevc() {
        let mut stream = Vec::new();
        stream.extend_from_slice(&nal(0x40, &[1]));
        stream.extend_from_slice(&nal(0x42, &[2]));
        stream.extend_from_slice(&nal(0x44, &[3]));
        stream.extend_from_slice(&nal(0x28, &[4]));
        let (tx, rx) = mpsc::sync_channel(8);
        pump_annexb(Cursor::new(stream), tx, true).unwrap();
        let first = rx.recv().unwrap();
        assert!(first.codec_config);
        let second = rx.recv().unwrap();
        assert!(second.keyframe);
    }

    /// One flushed AU must leave the pump without waiting for the next frame's
    /// start code. Otherwise glass-to-glass is one display refresh behind.
    #[test]
    fn flushed_au_emits_before_next_frame_arrives() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;
        use std::time::Duration;

        struct OneAuThenBlock {
            chunk: Option<Vec<u8>>,
            unblock: Arc<AtomicBool>,
        }
        impl std::io::Read for OneAuThenBlock {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if let Some(data) = self.chunk.take() {
                    buf[..data.len()].copy_from_slice(&data);
                    return Ok(data.len());
                }
                while !self.unblock.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(5));
                }
                Ok(0)
            }
        }

        let mut stream = Vec::new();
        stream.extend_from_slice(&nal(0x67, &[0x42]));
        stream.extend_from_slice(&nal(0x68, &[0xCE]));
        stream.extend_from_slice(&nal(0x65, &[0xAA]));
        let unblock = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(8);
        let reader = OneAuThenBlock {
            chunk: Some(stream),
            unblock: unblock.clone(),
        };
        let pump = thread::spawn(move || pump_annexb(reader, tx, false));
        let first = rx
            .recv_timeout(Duration::from_millis(400))
            .expect("codec-config should emit on the flushed AU, not on EOF");
        assert!(first.codec_config);
        let second = rx
            .recv_timeout(Duration::from_millis(400))
            .expect("IDR should emit on the flushed AU, not wait for the next frame");
        assert!(second.keyframe);
        unblock.store(true, Ordering::SeqCst);
        pump.join().unwrap().unwrap();
    }

    /// Windows pipes often return 4–8 KB even mid-AU. Flushing on the first
    /// short read truncated the slice. Idle-flush must wait until the writer
    /// has gone quiet after the last byte.
    #[test]
    fn chunked_au_assembles_before_idle_flush() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;
        use std::time::Duration;

        struct ChunkThenBlock {
            data: Vec<u8>,
            off: usize,
            chunk: usize,
            unblock: Arc<AtomicBool>,
        }
        impl std::io::Read for ChunkThenBlock {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.off >= self.data.len() {
                    while !self.unblock.load(Ordering::SeqCst) {
                        thread::sleep(Duration::from_millis(5));
                    }
                    return Ok(0);
                }
                let n = self
                    .chunk
                    .min(self.data.len() - self.off)
                    .min(buf.len());
                buf[..n].copy_from_slice(&self.data[self.off..self.off + n]);
                self.off += n;
                Ok(n)
            }
        }

        let mut stream = Vec::new();
        stream.extend_from_slice(&nal(0x67, &[0x42]));
        stream.extend_from_slice(&nal(0x68, &[0xCE]));
        stream.extend_from_slice(&nal(0x65, &[0xAA]));
        let unblock = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(8);
        let reader = ChunkThenBlock {
            data: stream,
            off: 0,
            chunk: 8,
            unblock: unblock.clone(),
        };
        let pump = thread::spawn(move || pump_annexb(reader, tx, false));
        let first = rx
            .recv_timeout(Duration::from_millis(400))
            .expect("codec-config after the chunked AU goes idle");
        assert!(first.codec_config);
        let second = rx
            .recv_timeout(Duration::from_millis(400))
            .expect("IDR after the chunked AU goes idle");
        assert!(second.keyframe);
        unblock.store(true, Ordering::SeqCst);
        pump.join().unwrap().unwrap();
    }

    #[test]
    fn read_buffer_matches_ffmpeg_pipe_so_full_reads_spin() {
        assert_eq!(
            read_buffer_bytes(),
            crate::session_policy::ffmpeg_pipe_buffer_bytes() as usize
        );
        assert!(read_buffer_bytes() >= 16 * 1024);
        assert!(read_buffer_bytes() <= 128 * 1024);
    }

    #[test]
    fn quiet_hint_flushes_without_waiting_idle() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;

        struct OneAuThenBlock {
            chunk: Option<Vec<u8>>,
            unblock: Arc<AtomicBool>,
        }
        impl std::io::Read for OneAuThenBlock {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if let Some(data) = self.chunk.take() {
                    buf[..data.len()].copy_from_slice(&data);
                    return Ok(data.len());
                }
                while !self.unblock.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(5));
                }
                Ok(0)
            }
        }

        let mut stream = Vec::new();
        stream.extend_from_slice(&nal(0x67, &[0x42]));
        stream.extend_from_slice(&nal(0x68, &[0xCE]));
        stream.extend_from_slice(&nal(0x65, &[0xAA]));
        let unblock = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(8);
        let reader = OneAuThenBlock {
            chunk: Some(stream),
            unblock: unblock.clone(),
        };
        // Idle is 400 ms — Quiet must emit well before that.
        let pump = thread::spawn(move || {
            pump_annexb_with_available_idle(reader, tx, false, |_| Some(0), Duration::from_millis(400))
        });
        let first = rx
            .recv_timeout(Duration::from_millis(80))
            .expect("codec-config on Quiet, not 400 ms idle");
        assert!(first.codec_config);
        let second = rx
            .recv_timeout(Duration::from_millis(80))
            .expect("IDR on Quiet, not 400 ms idle");
        assert!(second.keyframe);
        unblock.store(true, Ordering::SeqCst);
        pump.join().unwrap().unwrap();
    }

    #[test]
    fn recv_bootstrap_times_out_without_idr() {
        let (tx, rx) = mpsc::sync_channel(4);
        tx.send(EncodedPacket {
            data: nal(0x67, &[1]),
            keyframe: false,
            codec_config: true,
        })
        .unwrap();
        drop(tx);
        let err = recv_bootstrap(&rx, Duration::from_millis(20), false).unwrap_err();
        assert!(format!("{err:#}").contains("codec-config + IDR"));
    }

    #[tokio::test]
    async fn async_bootstrap_wakes_without_polling() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        tx.try_send(EncodedPacket {
            data: nal(0x67, &[1]),
            keyframe: false,
            codec_config: true,
        })
        .unwrap();
        tx.try_send(EncodedPacket {
            data: nal(0x65, &[2]),
            keyframe: true,
            codec_config: false,
        })
        .unwrap();
        drop(tx);
        let pkts = recv_bootstrap_async(&mut rx, Duration::from_millis(200), false)
            .await
            .unwrap();
        assert_eq!(pkts.len(), 2);
        assert!(pkts[0].codec_config);
        assert!(pkts[1].keyframe);
    }
}
