use anyhow::{bail, Result};
use std::io::Read;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

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
            19 | 20 | 21 => NalRole::Idr,
            0..=31 => NalRole::Vcl,
            _ => NalRole::Other,
        }
    } else {
        match h264_nal_type(nal) {
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
    mut stdout: impl Read,
    tx: mpsc::SyncSender<EncodedPacket>,
    hevc: bool,
) -> Result<()> {
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
        acc.extend_from_slice(&buf[..n]);
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
    Ok(())
}

fn ingest_nal(
    nal: Vec<u8>,
    hevc: bool,
    au: &mut Vec<u8>,
    au_has_vcl: &mut bool,
    au_key: &mut bool,
    sps_pps: &mut Vec<u8>,
    sent_cfg: &mut bool,
    tx: &mpsc::SyncSender<EncodedPacket>,
    drop_until_key: &mut bool,
) {
    let role = nal_role(&nal, hevc);
    if role == NalRole::ParameterSet {
        if starts_parameter_set_group(&nal, hevc) {
            sps_pps.clear();
        }
        sps_pps.extend_from_slice(&nal);
        return;
    }
    if !*sent_cfg && !sps_pps.is_empty() {
        let _ = tx.send(EncodedPacket {
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
    fn h264_roles() {
        assert_eq!(nal_role(&nal(0x67, &[]), false), NalRole::ParameterSet);
        assert_eq!(nal_role(&nal(0x68, &[]), false), NalRole::ParameterSet);
        assert_eq!(nal_role(&nal(0x65, &[]), false), NalRole::Idr);
        assert_eq!(nal_role(&nal(0x41, &[]), false), NalRole::Vcl);
        assert_eq!(nal_role(&nal(0x09, &[]), false), NalRole::Aud);
    }

    #[test]
    fn hevc_roles() {
        assert_eq!(nal_role(&nal(0x40, &[]), true), NalRole::ParameterSet); // VPS 32
        assert_eq!(nal_role(&nal(0x42, &[]), true), NalRole::ParameterSet); // SPS 33
        assert_eq!(nal_role(&nal(0x44, &[]), true), NalRole::ParameterSet); // PPS 34
        assert_eq!(nal_role(&nal(0x28, &[]), true), NalRole::Idr); // IDR_N_LP 20
        assert_eq!(nal_role(&nal(0x26, &[]), true), NalRole::Idr); // IDR_W_RADL 19
        assert_eq!(nal_role(&nal(0x02, &[]), true), NalRole::Vcl);
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
}
