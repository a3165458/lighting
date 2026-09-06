//! HEVC VPS/SPS rewrite for glass latency.
//!
//! NVENC/QSV/AMF write `sps_max_dec_pic_buffering_minus1` from the level
//! (often 4–6) and a non-zero `sps_max_num_reorder_pics`. Android then holds
//! pictures even with IPPP / `-refs 1`. Cap DPB at two pictures (current +
//! one reference) and force reorder/latency to 0.

fn start_code_len(nal: &[u8]) -> usize {
    if nal.len() >= 4 && nal[0] == 0 && nal[1] == 0 && nal[2] == 0 && nal[3] == 1 {
        4
    } else if nal.len() >= 3 && nal[0] == 0 && nal[1] == 0 && nal[2] == 1 {
        3
    } else {
        0
    }
}

fn hevc_nal_type(nal: &[u8]) -> u8 {
    let i = start_code_len(nal);
    nal.get(i).map(|b| (b >> 1) & 0x3F).unwrap_or(0)
}

/// Annex-B NAL: rewrite VPS (32) / SPS (33) DPB fields. Other NALs and
/// parse failures are returned unchanged.
pub fn rewrite_low_latency(nal: Vec<u8>) -> Vec<u8> {
    match hevc_nal_type(&nal) {
        32 | 33 => match rewrite_ps_nal(&nal) {
            Some(out) if !out.is_empty() => out,
            _ => nal,
        },
        _ => nal,
    }
}

fn rewrite_ps_nal(nal: &[u8]) -> Option<Vec<u8>> {
    let prefix = start_code_len(nal);
    if prefix == 0 || nal.len() <= prefix + 2 {
        return None;
    }
    let header = &nal[prefix..prefix + 2];
    let rbsp = unescape_rbsp(&nal[prefix + 2..]);
    let rewritten = match hevc_nal_type(nal) {
        32 => rewrite_vps_rbsp(&rbsp)?,
        33 => rewrite_sps_rbsp(&rbsp)?,
        _ => return None,
    };
    let mut out = Vec::with_capacity(prefix + 2 + rewritten.len() + rewritten.len() / 2);
    out.extend_from_slice(&nal[..prefix]);
    out.extend_from_slice(header);
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
        if n > 32 || self.remaining() < n as usize {
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
}

#[derive(Clone)]
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
        let mut left = n;
        while left > 0 {
            let chunk = left.min(32);
            let shift = left - chunk;
            let part = if shift >= 32 { 0 } else { val >> shift };
            for i in (0..chunk).rev() {
                let bit = ((part >> i) & 1) as u8;
                self.acc = (self.acc << 1) | bit;
                self.nbits += 1;
                if self.nbits == 8 {
                    self.bytes.push(self.acc);
                    self.acc = 0;
                    self.nbits = 0;
                }
            }
            left -= chunk;
        }
    }
    fn ue(&mut self, val: u32) {
        let v = val.saturating_add(1);
        let leading = 31 - v.leading_zeros();
        self.u(leading, 0);
        self.u(leading + 1, v);
    }

    fn copy_u(&mut self, src: &mut Bits<'_>, n: u32) -> Option<()> {
        let mut left = n;
        while left > 0 {
            let chunk = left.min(32);
            let v = src.u(chunk)?;
            self.u(chunk, v);
            left -= chunk;
        }
        Some(())
    }
    fn copy_ue(&mut self, src: &mut Bits<'_>) -> Option<u32> {
        let v = src.ue()?;
        self.ue(v);
        Some(v)
    }

    fn finish(mut self) -> Vec<u8> {
        self.u(1, 1);
        if self.nbits != 0 {
            self.u((8 - self.nbits) as u32, 0);
        }
        self.bytes
    }
}

fn copy_ptl_common(src: &mut Bits<'_>, dst: &mut Writer) -> Option<()> {
    dst.copy_u(src, 2)?;
    dst.copy_u(src, 1)?;
    let idc = src.u(5)?;
    dst.u(5, idc);
    let mut compat = [false; 32];
    for flag in &mut compat {
        let b = src.u(1)?;
        dst.u(1, b);
        *flag = b == 1;
    }
    dst.copy_u(src, 1)?;
    dst.copy_u(src, 1)?;
    dst.copy_u(src, 1)?;
    dst.copy_u(src, 1)?;
    let check = |p: u32| idc == p || compat[p as usize];
    if check(4) || check(5) || check(6) || check(7) || check(8) || check(9) || check(10) {
        dst.copy_u(src, 9)?;
        if check(5) || check(9) || check(10) {
            dst.copy_u(src, 1)?;
            dst.copy_u(src, 33)?;
        } else {
            dst.copy_u(src, 34)?;
        }
    } else if check(2) {
        dst.copy_u(src, 7)?;
        dst.copy_u(src, 1)?;
        dst.copy_u(src, 35)?;
    } else {
        dst.copy_u(src, 43)?;
    }
    dst.copy_u(src, 1)?;
    Some(())
}

fn copy_ptl(src: &mut Bits<'_>, dst: &mut Writer, max_sub_layers: u32) -> Option<()> {
    if max_sub_layers == 0 || max_sub_layers > 8 {
        return None;
    }
    copy_ptl_common(src, dst)?;
    let level = src.u(8)?;
    // HEVC 5.2 = 156. 5.1 MaxLumaSr is 267e6 (1080p120); 2560x1440x120
    // is 442e6 and 2K tablets pick HEVC. Floor at 5.2.
    dst.u(8, level.max(156));
    let max_minus1 = max_sub_layers - 1;
    let mut profile_present = [false; 8];
    let mut level_present = [false; 8];
    for i in 0..max_minus1 {
        profile_present[i as usize] = src.u(1)? == 1;
        dst.u(1, u32::from(profile_present[i as usize]));
        level_present[i as usize] = src.u(1)? == 1;
        dst.u(1, u32::from(level_present[i as usize]));
    }
    if max_minus1 > 0 {
        for _ in max_minus1..8 {
            dst.copy_u(src, 2)?;
        }
    }
    for i in 0..max_minus1 {
        if profile_present[i as usize] {
            copy_ptl_common(src, dst)?;
        }
        if level_present[i as usize] {
            let lvl = src.u(8)?;
            dst.u(8, lvl.max(156));
        }
    }
    Some(())
}

fn rewrite_dpb_loop(
    src: &mut Bits<'_>,
    dst: &mut Writer,
    max_sub_layers: u32,
    present: bool,
) -> Option<()> {
    let start = if present {
        0
    } else {
        max_sub_layers.saturating_sub(1)
    };
    for _ in start..max_sub_layers {
        let dpb = src.ue()?;
        let _reorder = src.ue()?;
        let _latency = src.ue()?;
        dst.ue(dpb.min(1));
        dst.ue(0);
        dst.ue(0);
    }
    Some(())
}

fn copy_rest_without_trailing(src: &mut Bits<'_>, dst: &mut Writer) -> Option<()> {
    let mut bits = Vec::with_capacity(src.remaining());
    while src.remaining() > 0 {
        bits.push(src.u(1)?);
    }
    while bits.last() == Some(&0) {
        bits.pop();
    }
    if bits.last() == Some(&1) {
        bits.pop();
    }
    for b in bits {
        dst.u(1, b);
    }
    Some(())
}


fn copy_st_ref_pic_set(src: &mut Bits<'_>, dst: &mut Writer, idx: u32) -> Option<()> {
    if idx != 0 {
        let inter = src.u(1)?;
        dst.u(1, inter);
        if inter == 1 {
            return None;
        }
    }
    let neg = dst.copy_ue(src)?;
    let pos = dst.copy_ue(src)?;
    if neg > 16 || pos > 16 {
        return None;
    }
    for _ in 0..neg {
        dst.copy_ue(src)?;
        dst.copy_u(src, 1)?;
    }
    for _ in 0..pos {
        dst.copy_ue(src)?;
        dst.copy_u(src, 1)?;
    }
    Some(())
}

fn skip_hevc_sub_layer_hrd(
    src: &mut Bits<'_>,
    cpb_cnt_minus1: u32,
    sub_pic: u32,
) -> Option<()> {
    if cpb_cnt_minus1 > 31 {
        return None;
    }
    for _ in 0..=cpb_cnt_minus1 {
        src.ue()?;
        src.ue()?;
        if sub_pic == 1 {
            src.ue()?;
            src.ue()?;
        }
        src.u(1)?;
    }
    Some(())
}

/// H.265 E.2.2. `common` is `commonInfPresentFlag`.
fn skip_hevc_hrd(src: &mut Bits<'_>, common: bool, max_sub_layers: u32) -> Option<()> {
    if max_sub_layers == 0 || max_sub_layers > 8 {
        return None;
    }
    // Subsequent VPS HRD sets reuse the previous nal/vcl flags. We only
    // rewrite the common-inf case (NVENC nhrd=1).
    if !common {
        return None;
    }
    let nal = src.u(1)?;
    let vcl = src.u(1)?;
    let mut sub_pic = 0;
    if nal == 1 || vcl == 1 {
        sub_pic = src.u(1)?;
        if sub_pic == 1 {
            src.u(8)?;
            src.u(5)?;
            src.u(1)?;
            src.u(5)?;
        }
        src.u(4)?;
        src.u(4)?;
        if sub_pic == 1 {
            src.u(4)?;
        }
        src.u(5)?;
        src.u(5)?;
        src.u(5)?;
    }
    for _ in 0..max_sub_layers {
        let general = src.u(1)?;
        let within = if general == 1 { 1 } else { src.u(1)? };
        let low_delay = if within == 1 {
            src.ue()?;
            0
        } else {
            src.u(1)?
        };
        let cpb_cnt = if low_delay == 1 { 0 } else { src.ue()? };
        if nal == 1 {
            skip_hevc_sub_layer_hrd(src, cpb_cnt, sub_pic)?;
        }
        if vcl == 1 {
            skip_hevc_sub_layer_hrd(src, cpb_cnt, sub_pic)?;
        }
    }
    Some(())
}

fn copy_vui_drop_timing(
    src: &mut Bits<'_>,
    dst: &mut Writer,
    max_sub_layers: u32,
) -> Option<()> {
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
    let vs = src.u(1)?;
    dst.u(1, vs);
    if vs == 1 {
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
    let chroma = src.u(1)?;
    dst.u(1, chroma);
    if chroma == 1 {
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
    }
    dst.copy_u(src, 1)?;
    dst.copy_u(src, 1)?;
    dst.copy_u(src, 1)?;
    let def_win = src.u(1)?;
    dst.u(1, def_win);
    if def_win == 1 {
        for _ in 0..4 {
            dst.copy_ue(src)?;
        }
    }
    let timing = src.u(1)?;
    // Same as H.264: NVENC inherits ddagrab's 8000 Hz / 25 fps timebase.
    // Android then paces like a movie. Drop vui_timing_info.
    dst.u(1, 0);
    if timing == 1 {
        src.u(32)?;
        src.u(32)?;
        let poc = src.u(1)?;
        if poc == 1 {
            src.ue()?;
        }
        let hrd = src.u(1)?;
        if hrd == 1 {
            skip_hevc_hrd(src, true, max_sub_layers)?;
        }
    }
    let restrict = src.u(1)?;
    dst.u(1, restrict);
    if restrict == 1 {
        dst.copy_u(src, 1)?;
        dst.copy_u(src, 1)?;
        dst.copy_u(src, 1)?;
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
    }
    Some(())
}

fn copy_sps_tail_drop_timing(
    src: &mut Bits<'_>,
    dst: &mut Writer,
    poc_lsb_bits: u32,
    max_sub_layers: u32,
) -> Option<()> {
    dst.copy_ue(src)?;
    dst.copy_ue(src)?;
    dst.copy_ue(src)?;
    dst.copy_ue(src)?;
    dst.copy_ue(src)?;
    dst.copy_ue(src)?;
    let scaling = src.u(1)?;
    dst.u(1, scaling);
    if scaling == 1 {
        let present = src.u(1)?;
        dst.u(1, present);
        if present == 1 {
            return None;
        }
    }
    dst.copy_u(src, 1)?;
    dst.copy_u(src, 1)?;
    let pcm = src.u(1)?;
    dst.u(1, pcm);
    if pcm == 1 {
        dst.copy_u(src, 4)?;
        dst.copy_u(src, 4)?;
        dst.copy_ue(src)?;
        dst.copy_ue(src)?;
        dst.copy_u(src, 1)?;
    }
    let num_st = dst.copy_ue(src)?;
    if num_st > 64 {
        return None;
    }
    for i in 0..num_st {
        copy_st_ref_pic_set(src, dst, i)?;
    }
    let lt = src.u(1)?;
    dst.u(1, lt);
    if lt == 1 {
        let n = dst.copy_ue(src)?;
        if n > 32 || poc_lsb_bits == 0 || poc_lsb_bits > 16 {
            return None;
        }
        for _ in 0..n {
            dst.copy_u(src, poc_lsb_bits)?;
            dst.copy_u(src, 1)?;
        }
    }
    dst.copy_u(src, 1)?;
    dst.copy_u(src, 1)?;
    let vui = src.u(1)?;
    dst.u(1, vui);
    if vui == 1 {
        copy_vui_drop_timing(src, dst, max_sub_layers)?;
    }
    copy_rest_without_trailing(src, dst)
}

fn rewrite_sps_rbsp(rbsp: &[u8]) -> Option<Vec<u8>> {
    if rbsp.len() < 4 {
        return None;
    }
    let mut src = Bits::new(rbsp);
    let mut dst = Writer::new();
    dst.copy_u(&mut src, 4)?;
    let max_sub_layers = src.u(3)? + 1;
    dst.u(3, max_sub_layers - 1);
    dst.copy_u(&mut src, 1)?;
    copy_ptl(&mut src, &mut dst, max_sub_layers)?;
    dst.copy_ue(&mut src)?;
    let chroma = dst.copy_ue(&mut src)?;
    if chroma == 3 {
        dst.copy_u(&mut src, 1)?;
    }
    dst.copy_ue(&mut src)?;
    dst.copy_ue(&mut src)?;
    let window = src.u(1)?;
    dst.u(1, window);
    if window == 1 {
        for _ in 0..4 {
            dst.copy_ue(&mut src)?;
        }
    }
    dst.copy_ue(&mut src)?;
    dst.copy_ue(&mut src)?;
    let log2_poc_lsb_minus4 = dst.copy_ue(&mut src)?;
    let ordering = src.u(1)?;
    dst.u(1, ordering);
    rewrite_dpb_loop(&mut src, &mut dst, max_sub_layers, ordering == 1)?;
    let tail_bit = src.bit;
    let tail_dst = dst.clone();
    if copy_sps_tail_drop_timing(
        &mut src,
        &mut dst,
        log2_poc_lsb_minus4.saturating_add(4),
        max_sub_layers,
    )
    .is_none()
    {
        src.bit = tail_bit;
        dst = tail_dst;
        copy_rest_without_trailing(&mut src, &mut dst)?;
    }
    Some(dst.finish())
}

fn rewrite_vps_rbsp(rbsp: &[u8]) -> Option<Vec<u8>> {
    if rbsp.len() < 4 {
        return None;
    }
    let mut src = Bits::new(rbsp);
    let mut dst = Writer::new();
    dst.copy_u(&mut src, 4)?;
    dst.copy_u(&mut src, 1)?;
    dst.copy_u(&mut src, 1)?;
    dst.copy_u(&mut src, 6)?;
    let max_sub_layers = src.u(3)? + 1;
    dst.u(3, max_sub_layers - 1);
    dst.copy_u(&mut src, 1)?;
    dst.copy_u(&mut src, 16)?;
    copy_ptl(&mut src, &mut dst, max_sub_layers)?;
    let ordering = src.u(1)?;
    dst.u(1, ordering);
    rewrite_dpb_loop(&mut src, &mut dst, max_sub_layers, ordering == 1)?;
    let tail_bit = src.bit;
    let tail_dst = dst.clone();
    if copy_vps_tail_drop_timing(&mut src, &mut dst, max_sub_layers).is_none() {
        src.bit = tail_bit;
        dst = tail_dst;
        copy_rest_without_trailing(&mut src, &mut dst)?;
    }
    Some(dst.finish())
}

fn copy_vps_tail_drop_timing(
    src: &mut Bits<'_>,
    dst: &mut Writer,
    max_sub_layers: u32,
) -> Option<()> {
    let max_layer_id = src.u(6)?;
    dst.u(6, max_layer_id);
    let sets_minus1 = dst.copy_ue(src)?;
    if sets_minus1 > 16 || max_layer_id > 63 {
        return None;
    }
    for _ in 1..=sets_minus1 {
        for _ in 0..=max_layer_id {
            dst.copy_u(src, 1)?;
        }
    }
    let timing = src.u(1)?;
    dst.u(1, 0);
    if timing == 1 {
        src.u(32)?;
        src.u(32)?;
        let poc = src.u(1)?;
        if poc == 1 {
            src.ue()?;
        }
        let nhrd = src.ue()?;
        if nhrd > 16 {
            return None;
        }
        for i in 0..nhrd {
            src.ue()?; // hrd_layer_set_idx
            let common = if i == 0 { true } else { src.u(1)? == 1 };
            skip_hevc_hrd(src, common, max_sub_layers)?;
        }
    }
    copy_rest_without_trailing(src, dst)
}

/// `(max_dec_pic_buffering_minus1, max_num_reorder_pics, max_latency_increase_plus1)`.
pub fn parse_dpb(nal: &[u8]) -> Option<(u32, u32, u32)> {
    let prefix = start_code_len(nal);
    if prefix == 0 || nal.len() <= prefix + 2 {
        return None;
    }
    let rbsp = unescape_rbsp(&nal[prefix + 2..]);
    let mut src = Bits::new(&rbsp);
    match hevc_nal_type(nal) {
        33 => {
            src.u(4)?;
            let max_sub_layers = src.u(3)? + 1;
            src.u(1)?;
            skip_ptl(&mut src, max_sub_layers)?;
            src.ue()?;
            let chroma = src.ue()?;
            if chroma == 3 {
                src.u(1)?;
            }
            src.ue()?;
            src.ue()?;
            if src.u(1)? == 1 {
                for _ in 0..4 {
                    src.ue()?;
                }
            }
            src.ue()?;
            src.ue()?;
            src.ue()?;
            let ordering = src.u(1)?;
            read_first_dpb(&mut src, max_sub_layers, ordering == 1)
        }
        32 => {
            src.u(4)?;
            src.u(1)?;
            src.u(1)?;
            src.u(6)?;
            let max_sub_layers = src.u(3)? + 1;
            src.u(1)?;
            src.u(16)?;
            skip_ptl(&mut src, max_sub_layers)?;
            let ordering = src.u(1)?;
            read_first_dpb(&mut src, max_sub_layers, ordering == 1)
        }
        _ => None,
    }
}

fn skip_ptl(src: &mut Bits<'_>, max_sub_layers: u32) -> Option<()> {
    let mut sink = Writer::new();
    copy_ptl(src, &mut sink, max_sub_layers)
}

fn read_first_dpb(
    src: &mut Bits<'_>,
    max_sub_layers: u32,
    present: bool,
) -> Option<(u32, u32, u32)> {
    let start = if present {
        0
    } else {
        max_sub_layers.saturating_sub(1)
    };
    let mut first = None;
    for _ in start..max_sub_layers {
        let dpb = src.ue()?;
        let reorder = src.ue()?;
        let latency = src.ue()?;
        if first.is_none() {
            first = Some((dpb, reorder, latency));
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nal_from_rbsp(header: &[u8], rbsp: &[u8]) -> Vec<u8> {
        let mut out = vec![0, 0, 0, 1];
        out.extend_from_slice(header);
        escape_rbsp(rbsp, &mut out);
        out
    }

    fn write_main_ptl(w: &mut Writer) {
        w.u(2, 0);
        w.u(1, 0);
        w.u(5, 1);
        for i in 0..32 {
            w.u(1, u32::from(i == 1));
        }
        w.u(1, 1);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 1);
        w.u(43, 0);
        w.u(1, 0);
        w.u(8, 120);
    }

    fn main_sps(dpb: u32, reorder: u32) -> Vec<u8> {
        let mut w = Writer::new();
        w.u(4, 0);
        w.u(3, 0);
        w.u(1, 1);
        write_main_ptl(&mut w);
        w.ue(0);
        w.ue(1);
        w.ue(1920);
        w.ue(1088);
        w.u(1, 0);
        w.ue(0);
        w.ue(0);
        w.ue(4);
        w.u(1, 1);
        w.ue(dpb);
        w.ue(reorder);
        w.ue(0);
        w.ue(0);
        w.ue(3);
        w.ue(0);
        w.ue(3);
        w.ue(0);
        w.ue(0);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 0);
        w.ue(0);
        w.u(1, 0);
        w.u(1, 1);
        w.u(1, 1);
        w.u(1, 0);
        w.u(1, 0);
        nal_from_rbsp(&[0x42, 0x01], &w.finish())
    }

    fn main_vps(dpb: u32, reorder: u32) -> Vec<u8> {
        let mut w = Writer::new();
        w.u(4, 0);
        w.u(1, 1);
        w.u(1, 1);
        w.u(6, 0);
        w.u(3, 0);
        w.u(1, 1);
        w.u(16, 0xffff);
        write_main_ptl(&mut w);
        w.u(1, 1);
        w.ue(dpb);
        w.ue(reorder);
        w.ue(0);
        w.u(6, 0);
        w.ue(0);
        w.u(1, 0);
        w.u(1, 0);
        nal_from_rbsp(&[0x40, 0x01], &w.finish())
    }

    #[test]
    fn leaves_non_ps_alone() {
        let pps = vec![0, 0, 0, 1, 0x44, 0x01, 1, 2, 3];
        assert_eq!(rewrite_low_latency(pps.clone()), pps);
        let slice = vec![0, 0, 0, 1, 0x02, 0x01, 9];
        assert_eq!(rewrite_low_latency(slice.clone()), slice);
    }

    #[test]
    fn sps_caps_dpb_and_clears_reorder() {
        let src = main_sps(5, 2);
        assert_eq!(parse_dpb(&src), Some((5, 2, 0)));
        let out = rewrite_low_latency(src);
        assert_eq!(parse_dpb(&out), Some((1, 0, 0)));
        let again = rewrite_low_latency(out.clone());
        assert_eq!(parse_dpb(&again), Some((1, 0, 0)));
    }

    fn sps_general_level(nal: &[u8]) -> Option<u32> {
        let prefix = start_code_len(nal);
        let rbsp = unescape_rbsp(&nal[prefix + 2..]);
        let mut src = Bits::new(&rbsp);
        src.u(4)?;
        src.u(3)?;
        src.u(1)?;
        let mut sink = Writer::new();
        copy_ptl_common(&mut src, &mut sink)?;
        src.u(8)
    }

    #[test]
    fn floors_hevc_level_to_5_2_so_2k120_is_not_tagged_60() {
        let src = main_sps(5, 2);
        assert_eq!(sps_general_level(&src), Some(120));
        let out = rewrite_low_latency(src);
        assert_eq!(sps_general_level(&out), Some(156));
        assert_eq!(parse_dpb(&out), Some((1, 0, 0)));
        assert_eq!(sps_general_level(&rewrite_low_latency(out.clone())), Some(156));
    }

    #[test]
    fn sps_keeps_all_intra_dpb() {
        let src = main_sps(0, 0);
        assert_eq!(parse_dpb(&src), Some((0, 0, 0)));
        let out = rewrite_low_latency(src);
        assert_eq!(parse_dpb(&out), Some((0, 0, 0)));
    }

    #[test]
    fn vps_caps_dpb_and_clears_reorder() {
        let src = main_vps(4, 1);
        assert_eq!(parse_dpb(&src), Some((4, 1, 0)));
        let out = rewrite_low_latency(src);
        assert_eq!(parse_dpb(&out), Some((1, 0, 0)));
    }

    fn nvenc_like_vps(dpb: u32, reorder: u32) -> Vec<u8> {
        let mut w = Writer::new();
        w.u(4, 0);
        w.u(1, 1);
        w.u(1, 1);
        w.u(6, 0);
        w.u(3, 0);
        w.u(1, 1);
        w.u(16, 0xffff);
        write_main_ptl(&mut w);
        w.u(1, 1);
        w.ue(dpb);
        w.ue(reorder);
        w.ue(0);
        w.u(6, 0);
        w.ue(0);
        w.u(1, 1);
        w.u(32, 1);
        w.u(32, 60);
        w.u(1, 0);
        w.ue(0);
        w.u(1, 0);
        nal_from_rbsp(&[0x40, 0x01], &w.finish())
    }

    #[test]
    fn drops_vps_movie_timing_so_decoder_does_not_pace() {
        let src = nvenc_like_vps(5, 2);
        assert_eq!(parse_dpb(&src), Some((5, 2, 0)));
        let out = rewrite_low_latency(src.clone());
        assert_ne!(out, src);
        assert_eq!(parse_dpb(&out), Some((1, 0, 0)));
        assert!(out.len() < src.len(), "vps timing_info should be gone");
        assert_eq!(rewrite_low_latency(out.clone()), out);
    }

    fn nvenc_like_sps(dpb: u32, reorder: u32) -> Vec<u8> {
        let mut w = Writer::new();
        w.u(4, 0);
        w.u(3, 0);
        w.u(1, 1);
        write_main_ptl(&mut w);
        w.ue(0);
        w.ue(1);
        w.ue(1920);
        w.ue(1088);
        w.u(1, 1);
        w.ue(0);
        w.ue(0);
        w.ue(0);
        w.ue(8);
        w.ue(0);
        w.ue(0);
        w.ue(4);
        w.u(1, 1);
        w.ue(dpb);
        w.ue(reorder);
        w.ue(0);
        w.ue(0); // log2_min_luma_coding_block_size_minus3
        w.ue(3);
        w.ue(0);
        w.ue(3);
        w.ue(2);
        w.ue(2);
        w.u(1, 0); // scaling_list
        w.u(1, 1); // amp
        w.u(1, 0); // sao off (ULL)
        w.u(1, 0); // pcm
        w.ue(1); // one short-term RPS
        w.ue(1); // num_negative_pics
        w.ue(0); // num_positive_pics
        w.ue(0); // delta_poc_s0_minus1
        w.u(1, 1); // used_by_curr_pic_s0_flag
        w.u(1, 0); // long_term_ref_pics_present
        w.u(1, 1); // temporal_mvp
        w.u(1, 1); // strong_intra_smoothing
        w.u(1, 1); // vui
        w.u(1, 0); // aspect
        w.u(1, 0); // overscan
        w.u(1, 0); // video_signal
        w.u(1, 0); // chroma_loc
        w.u(1, 0); // neutral_chroma
        w.u(1, 0); // field_seq
        w.u(1, 0); // frame_field_info
        w.u(1, 0); // default_display_window
        w.u(1, 1); // timing
        w.u(32, 1);
        w.u(32, 60);
        w.u(1, 0); // poc_proportional
        w.u(1, 0); // hrd
        w.u(1, 0); // bitstream_restriction
        w.u(1, 0); // extension
        nal_from_rbsp(&[0x42, 0x01], &w.finish())
    }

    #[test]
    fn garbage_sps_is_unchanged() {
        let bad = vec![0, 0, 0, 1, 0x42, 0x01, 0xff];
        assert_eq!(rewrite_low_latency(bad.clone()), bad);
    }

    #[test]
    fn nvenc_like_sps_with_rps_still_rewrites() {
        let src = nvenc_like_sps(5, 2);
        assert_eq!(parse_dpb(&src), Some((5, 2, 0)));
        let out = rewrite_low_latency(src.clone());
        assert_ne!(out, src);
        assert_eq!(parse_dpb(&out), Some((1, 0, 0)));
        let again = rewrite_low_latency(out.clone());
        assert_eq!(parse_dpb(&again), Some((1, 0, 0)));
    }

    #[test]
    fn drops_hevc_movie_timing_so_decoder_does_not_pace() {
        let src = nvenc_like_sps(16, 2);
        let out = rewrite_low_latency(src.clone());
        assert_ne!(out, src);
        assert_eq!(parse_dpb(&out), Some((1, 0, 0)));
        assert!(out.len() < src.len(), "timing_info should be gone");
        assert_eq!(rewrite_low_latency(out.clone()), out);
    }

    fn write_hevc_cbr_hrd(w: &mut Writer) {
        w.u(1, 1); // nal_hrd
        w.u(1, 0); // vcl_hrd
        w.u(1, 0); // sub_pic
        w.u(4, 4);
        w.u(4, 4);
        w.u(5, 23);
        w.u(5, 23);
        w.u(5, 23);
        w.u(1, 1); // fixed_pic_rate_general
        w.ue(0); // elemental_duration_in_tc_minus1
        w.ue(0); // cpb_cnt_minus1
        w.ue(1000);
        w.ue(1000);
        w.u(1, 1); // cbr_flag
    }

    fn nvenc_like_sps_cbr_hrd(dpb: u32, reorder: u32) -> Vec<u8> {
        let mut w = Writer::new();
        w.u(4, 0);
        w.u(3, 0);
        w.u(1, 1);
        write_main_ptl(&mut w);
        w.ue(0);
        w.ue(1);
        w.ue(1920);
        w.ue(1088);
        w.u(1, 1);
        w.ue(0);
        w.ue(0);
        w.ue(0);
        w.ue(8);
        w.ue(0);
        w.ue(0);
        w.ue(4);
        w.u(1, 1);
        w.ue(dpb);
        w.ue(reorder);
        w.ue(0);
        w.ue(0);
        w.ue(3);
        w.ue(0);
        w.ue(3);
        w.ue(2);
        w.ue(2);
        w.u(1, 0);
        w.u(1, 1);
        w.u(1, 0);
        w.u(1, 0);
        w.ue(1);
        w.ue(1);
        w.ue(0);
        w.ue(0);
        w.u(1, 1);
        w.u(1, 0);
        w.u(1, 1);
        w.u(1, 1);
        w.u(1, 1); // vui
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 0);
        w.u(1, 1); // timing
        w.u(32, 1);
        w.u(32, 60);
        w.u(1, 0);
        w.u(1, 1); // hrd
        write_hevc_cbr_hrd(&mut w);
        w.u(1, 0);
        w.u(1, 0);
        nal_from_rbsp(&[0x42, 0x01], &w.finish())
    }

    fn nvenc_like_vps_cbr_hrd(dpb: u32, reorder: u32) -> Vec<u8> {
        let mut w = Writer::new();
        w.u(4, 0);
        w.u(1, 1);
        w.u(1, 1);
        w.u(6, 0);
        w.u(3, 0);
        w.u(1, 1);
        w.u(16, 0xffff);
        write_main_ptl(&mut w);
        w.u(1, 1);
        w.ue(dpb);
        w.ue(reorder);
        w.ue(0);
        w.u(6, 0);
        w.ue(0);
        w.u(1, 1);
        w.u(32, 1);
        w.u(32, 60);
        w.u(1, 0);
        w.ue(1); // nhrd
        w.ue(0); // hrd_layer_set_idx
        write_hevc_cbr_hrd(&mut w);
        w.u(1, 0);
        nal_from_rbsp(&[0x40, 0x01], &w.finish())
    }

    #[test]
    fn drops_hevc_cbr_hrd_so_decoder_does_not_wait_cpb() {
        let src = nvenc_like_sps_cbr_hrd(16, 2);
        assert_eq!(parse_dpb(&src), Some((16, 2, 0)));
        let out = rewrite_low_latency(src.clone());
        assert_ne!(out, src);
        assert_eq!(parse_dpb(&out), Some((1, 0, 0)));
        assert_eq!(out, rewrite_low_latency(nvenc_like_sps(16, 2)));
        assert_eq!(rewrite_low_latency(out.clone()), out);
    }

    #[test]
    fn drops_vps_cbr_hrd_so_decoder_does_not_wait_cpb() {
        let src = nvenc_like_vps_cbr_hrd(5, 2);
        assert_eq!(parse_dpb(&src), Some((5, 2, 0)));
        let out = rewrite_low_latency(src.clone());
        assert_ne!(out, src);
        assert_eq!(parse_dpb(&out), Some((1, 0, 0)));
        assert_eq!(out, rewrite_low_latency(nvenc_like_vps(5, 2)));
        assert_eq!(rewrite_low_latency(out.clone()), out);
    }
}
