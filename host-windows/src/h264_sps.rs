//! Moonlight/WebRTC-style H.264 SPS rewrite for glass latency.
//!
//! NVENC/QSV/AMF often write `num_ref_frames` and VUI
//! `max_dec_frame_buffering` from the level (4–16). Android decoders then
//! hold decoded pictures even when the bitstream has no B-frames.
//! Moonlight `decoder-errata.txt`: set both to 1.

fn start_code_len(nal: &[u8]) -> usize {
    if nal.len() >= 4 && nal[0] == 0 && nal[1] == 0 && nal[2] == 0 && nal[3] == 1 {
        4
    } else if nal.len() >= 3 && nal[0] == 0 && nal[1] == 0 && nal[2] == 1 {
        3
    } else {
        0
    }
}

fn h264_nal_type(nal: &[u8]) -> u8 {
    let i = start_code_len(nal);
    nal.get(i).map(|b| b & 0x1F).unwrap_or(0)
}

/// Annex-B NAL: rewrite an SPS so the decoder DPB is one picture.
/// Non-SPS NALs and parse failures are returned unchanged.
pub fn rewrite_low_latency(nal: Vec<u8>) -> Vec<u8> {
    if h264_nal_type(&nal) != 7 {
        return nal;
    }
    match rewrite_sps_nal(&nal) {
        Some(out) if !out.is_empty() => out,
        _ => nal,
    }
}

fn rewrite_sps_nal(nal: &[u8]) -> Option<Vec<u8>> {
    let prefix = start_code_len(nal);
    if prefix == 0 || nal.len() <= prefix + 1 {
        return None;
    }
    let header = nal[prefix];
    let rbsp = unescape_rbsp(&nal[prefix + 1..]);
    let rewritten = rewrite_sps_rbsp(&rbsp)?;
    let mut out = Vec::with_capacity(prefix + 1 + rewritten.len() + rewritten.len() / 2);
    out.extend_from_slice(&nal[..prefix]);
    out.push(header);
    escape_rbsp(&rewritten, &mut out);
    Some(out)
}

fn unescape_rbsp(ebsp: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ebsp.len());
    let mut i = 0;
    while i < ebsp.len() {
        if i + 2 < ebsp.len() && ebsp[i] == 0 && ebsp[i + 1] == 0 && ebsp[i + 2] == 3 {
            out.push(0);
            out.push(0);
            i += 3;
            continue;
        }
        out.push(ebsp[i]);
        i += 1;
    }
    out
}

fn escape_rbsp(rbsp: &[u8], out: &mut Vec<u8>) {
    let mut zeros = 0u8;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(b);
        zeros = if b == 0 { zeros.saturating_add(1) } else { 0 };
    }
}

struct Bits<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_mul(8).saturating_sub(self.bit)
    }

    fn u(&mut self, n: u32) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        if self.remaining() < n as usize {
            return None;
        }
        let mut v = 0u32;
        for _ in 0..n {
            let byte = self.data[self.bit / 8];
            let shift = 7 - (self.bit % 8);
            v = (v << 1) | ((byte >> shift) as u32 & 1);
            self.bit += 1;
        }
        Some(v)
    }

    fn ue(&mut self) -> Option<u32> {
        let mut leading = 0u32;
        loop {
            match self.u(1)? {
                0 => leading += 1,
                _ => break,
            }
            if leading > 31 {
                return None;
            }
        }
        let suffix = self.u(leading)?;
        Some(((1u32 << leading) - 1).saturating_add(suffix))
    }

    fn se(&mut self) -> Option<i32> {
        let k = self.ue()?;
        let n = ((k + 1) / 2) as i32;
        if k % 2 == 0 {
            Some(-n)
        } else {
            Some(n)
        }
    }
}

struct Writer {
    bytes: Vec<u8>,
    acc: u8,
    nbits: u8,
}

impl Writer {
    fn new() -> Self {
        Self {
            bytes: Vec::new(),
            acc: 0,
            nbits: 0,
        }
    }

    fn u(&mut self, n: u32, val: u32) {
        for i in (0..n).rev() {
            let bit = ((val >> i) & 1) as u8;
            self.acc = (self.acc << 1) | bit;
            self.nbits += 1;
            if self.nbits == 8 {
                self.bytes.push(self.acc);
                self.acc = 0;
                self.nbits = 0;
            }
        }
    }

    fn ue(&mut self, val: u32) {
        let v = val.saturating_add(1);
        let leading = 31 - v.leading_zeros();
        self.u(leading, 0);
        self.u(leading + 1, v);
    }

    fn se(&mut self, val: i32) {
        let encoded = if val <= 0 {
            (-val as u32) * 2
        } else {
            (val as u32) * 2 - 1
        };
        self.ue(encoded);
    }

    fn copy_u(&mut self, src: &mut Bits<'_>, n: u32) -> Option<()> {
        let v = src.u(n)?;
        self.u(n, v);
        Some(())
    }

    fn copy_ue(&mut self, src: &mut Bits<'_>) -> Option<u32> {
        let v = src.ue()?;
        self.ue(v);
        Some(v)
    }

    fn copy_se(&mut self, src: &mut Bits<'_>) -> Option<()> {
        let v = src.se()?;
        self.se(v);
        Some(())
    }

    fn finish(mut self) -> Vec<u8> {
        self.u(1, 1);
        if self.nbits != 0 {
            self.u((8 - self.nbits) as u32, 0);
        }
        self.bytes
    }
}

fn rewrite_sps_rbsp(rbsp: &[u8]) -> Option<Vec<u8>> {
    if rbsp.len() < 4 {
        return None;
    }
    let mut src = Bits::new(rbsp);
    let mut dst = Writer::new();

    let profile = src.u(8)?;
    dst.u(8, profile);
    dst.copy_u(&mut src, 8)?;
    dst.copy_u(&mut src, 8)?;
    dst.copy_ue(&mut src)?;

    let mut chroma_format_idc = 1u32;
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format_idc = dst.copy_ue(&mut src)?;
        if chroma_format_idc == 3 {
            dst.copy_u(&mut src, 1)?;
        }
        dst.copy_ue(&mut src)?;
        dst.copy_ue(&mut src)?;
        dst.copy_u(&mut src, 1)?;
        let scaling = src.u(1)?;
        dst.u(1, scaling);
        if scaling == 1 {
            skip_scaling_lists(&mut src, &mut dst, chroma_format_idc)?;
        }
    }

    dst.copy_ue(&mut src)?;
    let poc = dst.copy_ue(&mut src)?;
    if poc == 0 {
        dst.copy_ue(&mut src)?;
    } else if poc == 1 {
        dst.copy_u(&mut src, 1)?;
        dst.copy_se(&mut src)?;
        dst.copy_se(&mut src)?;
        let n = dst.copy_ue(&mut src)?;
        for _ in 0..n {
            dst.copy_se(&mut src)?;
        }
    }
    let _old_refs = src.ue()?;
    dst.ue(1);
    dst.copy_u(&mut src, 1)?;
    dst.copy_ue(&mut src)?;
    dst.copy_ue(&mut src)?;
    let frame_mbs_only = src.u(1)?;
    dst.u(1, frame_mbs_only);
    if frame_mbs_only == 0 {
        dst.copy_u(&mut src, 1)?;
    }
    dst.copy_u(&mut src, 1)?;
    let crop = src.u(1)?;
    dst.u(1, crop);
    if crop == 1 {
        for _ in 0..4 {
            dst.copy_ue(&mut src)?;
        }
    }

    let vui_present = src.u(1)?;
    dst.u(1, 1);
    if vui_present == 1 {
        copy_vui_force_restriction(&mut src, &mut dst)?;
    } else {
        write_minimal_vui(&mut dst);
    }

    Some(dst.finish())
}

fn skip_scaling_lists(src: &mut Bits<'_>, dst: &mut Writer, chroma_format_idc: u32) -> Option<()> {
    let count = if chroma_format_idc != 3 { 8 } else { 12 };
    for i in 0..count {
        let present = src.u(1)?;
        dst.u(1, present);
        if present == 1 {
            let size = if i < 6 { 16 } else { 64 };
            copy_scaling_list(src, dst, size)?;
        }
    }
    Some(())
}

fn copy_scaling_list(src: &mut Bits<'_>, dst: &mut Writer, size: usize) -> Option<()> {
    let mut last_scale = 8i32;
    let mut next_scale = 8i32;
    for j in 0..size {
        if next_scale != 0 {
            let delta = src.se()?;
            dst.se(delta);
            next_scale = (last_scale + delta + 256) % 256;
            if j == 0 && next_scale == 0 {
                break;
            }
        }
        last_scale = if next_scale == 0 { last_scale } else { next_scale };
    }
    Some(())
}

fn copy_vui_force_restriction(src: &mut Bits<'_>, dst: &mut Writer) -> Option<()> {
    let ar = src.u(1)?;
    dst.u(1, ar);
    if ar == 1 {
        let idc = src.u(8)?;
        dst.u(8, idc);
        if idc == 255 {
            dst.copy_u(src, 16)?;
            dst.copy_u(src, 16)?;
        }
    }
    let overscan = src.u(1)?;
    dst.u(1, overscan);
    if overscan == 1 {
        dst.copy_u(src, 1)?;
    }
    let video_signal = src.u(1)?;
    dst.u(1, video_signal);
    if video_signal == 1 {
        dst.copy_u(src, 3)?;
        dst.copy_u(src, 1)?;
        let colour = src.u(1)?;
        dst.u(1, colour);
        if colour == 1 {
            dst.copy_u(src, 8)?;
            dst.copy_u(src, 8)?;
            dst.copy_u(src, 8)?;
        }
    }
    let chroma_loc = src.u(1)?;
    dst.u(1, chroma_loc);
    if chroma_loc == 1 {
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
    }
    let timing = src.u(1)?;
    dst.u(1, timing);
    if timing == 1 {
        dst.copy_u(src, 32)?;
        dst.copy_u(src, 32)?;
        dst.copy_u(src, 1)?;
    }
    let nal_hrd = src.u(1)?;
    dst.u(1, nal_hrd);
    if nal_hrd == 1 {
        copy_hrd(src, dst)?;
    }
    let vcl_hrd = src.u(1)?;
    dst.u(1, vcl_hrd);
    if vcl_hrd == 1 {
        copy_hrd(src, dst)?;
    }
    if nal_hrd == 1 || vcl_hrd == 1 {
        dst.copy_u(src, 1)?;
    }
    dst.copy_u(src, 1)?;

    let restriction = src.u(1).unwrap_or(0);
    dst.u(1, 1);
    if restriction == 1 {
        dst.copy_u(src, 1)?;
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
        let _reorder = src.ue()?;
        let _dpb = src.ue()?;
        dst.ue(0);
        dst.ue(1);
    } else {
        write_restriction_block(dst);
    }
    Some(())
}

fn copy_hrd(src: &mut Bits<'_>, dst: &mut Writer) -> Option<()> {
    let cpb_cnt = dst.copy_ue(src)?;
    dst.copy_u(src, 4)?;
    dst.copy_u(src, 4)?;
    for _ in 0..=cpb_cnt {
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
        dst.copy_u(src, 1)?;
    }
    dst.copy_u(src, 5)?;
    dst.copy_u(src, 5)?;
    dst.copy_u(src, 5)?;
    dst.copy_u(src, 5)?;
    Some(())
}

fn write_minimal_vui(dst: &mut Writer) {
    dst.u(1, 0);
    dst.u(1, 0);
    dst.u(1, 0);
    dst.u(1, 0);
    dst.u(1, 0);
    dst.u(1, 0);
    dst.u(1, 0);
    dst.u(1, 0);
    dst.u(1, 1);
    write_restriction_block(dst);
}

fn write_restriction_block(dst: &mut Writer) {
    dst.u(1, 1);
    dst.ue(2);
    dst.ue(1);
    dst.ue(16);
    dst.ue(16);
    dst.ue(0);
    dst.ue(1);
}

/// `(max_num_ref_frames, Some((max_num_reorder_frames, max_dec_frame_buffering)))`.
pub fn parse_dpb(nal: &[u8]) -> Option<(u32, Option<(u32, u32)>)> {
    if h264_nal_type(nal) != 7 {
        return None;
    }
    let prefix = start_code_len(nal);
    let rbsp = unescape_rbsp(&nal[prefix + 1..]);
    let mut src = Bits::new(&rbsp);
    let profile = src.u(8)?;
    src.u(8)?;
    src.u(8)?;
    src.ue()?;
    let mut chroma_format_idc = 1u32;
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma_format_idc = src.ue()?;
        if chroma_format_idc == 3 {
            src.u(1)?;
        }
        src.ue()?;
        src.ue()?;
        src.u(1)?;
        if src.u(1)? == 1 {
            let count = if chroma_format_idc != 3 { 8 } else { 12 };
            for i in 0..count {
                if src.u(1)? == 1 {
                    let size = if i < 6 { 16 } else { 64 };
                    let mut last_scale = 8i32;
                    let mut next_scale = 8i32;
                    for j in 0..size {
                        if next_scale != 0 {
                            let delta = src.se()?;
                            next_scale = (last_scale + delta + 256) % 256;
                            if j == 0 && next_scale == 0 {
                                break;
                            }
                        }
                        last_scale = if next_scale == 0 { last_scale } else { next_scale };
                    }
                }
            }
        }
    }
    src.ue()?;
    let poc = src.ue()?;
    if poc == 0 {
        src.ue()?;
    } else if poc == 1 {
        src.u(1)?;
        src.se()?;
        src.se()?;
        let n = src.ue()?;
        for _ in 0..n {
            src.se()?;
        }
    }
    let refs = src.ue()?;
    src.u(1)?;
    src.ue()?;
    src.ue()?;
    let frame_mbs_only = src.u(1)?;
    if frame_mbs_only == 0 {
        src.u(1)?;
    }
    src.u(1)?;
    if src.u(1)? == 1 {
        for _ in 0..4 {
            src.ue()?;
        }
    }
    if src.u(1)? == 0 {
        return Some((refs, None));
    }
    if src.u(1)? == 1 {
        let idc = src.u(8)?;
        if idc == 255 {
            src.u(16)?;
            src.u(16)?;
        }
    }
    if src.u(1)? == 1 {
        src.u(1)?;
    }
    if src.u(1)? == 1 {
        src.u(3)?;
        src.u(1)?;
        if src.u(1)? == 1 {
            src.u(8)?;
            src.u(8)?;
            src.u(8)?;
        }
    }
    if src.u(1)? == 1 {
        src.ue()?;
        src.ue()?;
    }
    if src.u(1)? == 1 {
        src.u(32)?;
        src.u(32)?;
        src.u(1)?;
    }
    let nal_hrd = src.u(1)?;
    if nal_hrd == 1 {
        skip_hrd(&mut src)?;
    }
    let vcl_hrd = src.u(1)?;
    if vcl_hrd == 1 {
        skip_hrd(&mut src)?;
    }
    if nal_hrd == 1 || vcl_hrd == 1 {
        src.u(1)?;
    }
    src.u(1)?;
    if src.u(1)? == 0 {
        return Some((refs, None));
    }
    src.u(1)?;
    src.ue()?;
    src.ue()?;
    src.ue()?;
    src.ue()?;
    let reorder = src.ue()?;
    let dpb = src.ue()?;
    Some((refs, Some((reorder, dpb))))
}

fn skip_hrd(src: &mut Bits<'_>) -> Option<()> {
    let cpb_cnt = src.ue()?;
    src.u(4)?;
    src.u(4)?;
    for _ in 0..=cpb_cnt {
        src.ue()?;
        src.ue()?;
        src.u(1)?;
    }
    src.u(5)?;
    src.u(5)?;
    src.u(5)?;
    src.u(5)?;
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nal_from_rbsp(header: u8, rbsp: &[u8]) -> Vec<u8> {
        let mut out = vec![0, 0, 0, 1, header];
        escape_rbsp(rbsp, &mut out);
        out
    }

    fn baseline_sps(refs: u32, vui: bool) -> Vec<u8> {
        let mut w = Writer::new();
        w.u(8, 66);
        w.u(8, 0);
        w.u(8, 42);
        w.ue(0);
        w.ue(0);
        w.ue(2);
        w.ue(refs);
        w.u(1, 0);
        w.ue(119);
        w.ue(67);
        w.u(1, 1);
        w.u(1, 1);
        w.u(1, 1);
        w.ue(0);
        w.ue(0);
        w.ue(0);
        w.ue(4);
        if vui {
            w.u(1, 1);
            write_minimal_vui(&mut w);
        } else {
            w.u(1, 0);
        }
        nal_from_rbsp(0x67, &w.finish())
    }

    fn high_sps(refs: u32) -> Vec<u8> {
        let mut w = Writer::new();
        w.u(8, 100);
        w.u(8, 0);
        w.u(8, 42);
        w.ue(0);
        w.ue(1);
        w.ue(0);
        w.ue(0);
        w.u(1, 0);
        w.u(1, 0);
        w.ue(0);
        w.ue(0);
        w.ue(4);
        w.ue(refs);
        w.u(1, 0);
        w.ue(119);
        w.ue(67);
        w.u(1, 1);
        w.u(1, 1);
        w.u(1, 0);
        w.u(1, 0);
        nal_from_rbsp(0x67, &w.finish())
    }

    #[test]
    fn leaves_non_sps_alone() {
        let pps = vec![0, 0, 0, 1, 0x68, 1, 2, 3];
        assert_eq!(rewrite_low_latency(pps.clone()), pps);
        let idr = vec![0, 0, 0, 1, 0x65, 9];
        assert_eq!(rewrite_low_latency(idr.clone()), idr);
    }

    #[test]
    fn baseline_without_vui_gets_one_frame_dpb() {
        let src = baseline_sps(16, false);
        assert_eq!(parse_dpb(&src), Some((16, None)));
        let out = rewrite_low_latency(src);
        assert_eq!(parse_dpb(&out), Some((1, Some((0, 1)))));
        let again = rewrite_low_latency(out.clone());
        assert_eq!(parse_dpb(&again), Some((1, Some((0, 1)))));
    }

    #[test]
    fn baseline_with_vui_rewrites_restriction() {
        let src = baseline_sps(8, true);
        assert_eq!(parse_dpb(&src), Some((8, Some((0, 1)))));
        let out = rewrite_low_latency(src);
        assert_eq!(parse_dpb(&out), Some((1, Some((0, 1)))));
    }

    #[test]
    fn high_profile_refs_drop_to_one() {
        let src = high_sps(4);
        assert_eq!(parse_dpb(&src).map(|p| p.0), Some(4));
        let out = rewrite_low_latency(src);
        assert_eq!(parse_dpb(&out), Some((1, Some((0, 1)))));
    }

    #[test]
    fn garbage_sps_is_unchanged() {
        let bad = vec![0, 0, 0, 1, 0x67, 0xff];
        assert_eq!(rewrite_low_latency(bad.clone()), bad);
    }
}
