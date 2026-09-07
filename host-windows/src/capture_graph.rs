//! Capture/scale graphs for FFmpeg Desktop Duplication.
//!
//! Prefer GPU-resident paths (no `hwdownload`) so we cut latency without
//! lowering bitrate/fps/resolution. CPU graphs stay as fallbacks and use
//! `bilinear` (not `fast_bilinear`) when scaling is required.

/// `output_index` is local to this DXGI adapter, not the visible monitor list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DxgiCapture {
    pub adapter_index: u32,
    pub output_index: u32,
    /// DXGI VendorId: NVIDIA 0x10DE, Intel 0x8086, AMD 0x1002.
    pub vendor_id: u32,
}

impl DxgiCapture {
    /// Give ddagrab a D3D11 device from the adapter that owns the selected output.
    pub fn device_args(self) -> [String; 4] {
        [
            "-init_hw_device".into(),
            format!("d3d11va=capture:{}", self.adapter_index),
            "-filter_hw_device".into(),
            "capture".into(),
        ]
    }

    /// Derive CUDA from the D3D11 capture device (same adapter). `cuda:0`
    /// is the first CUDA GPU, not DXGI adapter_index — wrong on dual-GPU
    /// laptops. Only pass this when the filter graph actually uses CUDA;
    /// a failed CUDA init would kill identity `ddagrab` too.
    pub fn cuda_device_args(self) -> [String; 2] {
        ["-init_hw_device".into(), "cuda=cuda@capture".into()]
    }

    /// Derive QSV from the D3D11 capture device. Same dual-GPU trap as
    /// CUDA: `qsv:0` is the first Intel adapter, not DXGI adapter_index.
    pub fn qsv_device_args(self) -> [String; 2] {
        ["-init_hw_device".into(), "qsv=qsv@capture".into()]
    }

    /// Derive AMF from the D3D11 capture device for `vpp_amf`.
    pub fn amf_device_args(self) -> [String; 2] {
        ["-init_hw_device".into(), "amf=amf@capture".into()]
    }
}

/// Extra `-init_hw_device` entries this filter graph needs besides D3D11.
pub fn extra_hw_device_args(capture: DxgiCapture, graph: &str) -> Vec<String> {
    let mut extra = Vec::new();
    if graph.contains("cuda") {
        extra.extend(capture.cuda_device_args());
    }
    if graph.contains("derive_device=qsv") || graph.contains("scale_qsv") {
        extra.extend(capture.qsv_device_args());
    }
    if graph.contains("vpp_amf") || graph.contains("derive_device=amf") {
        extra.extend(capture.amf_device_args());
    }
    extra
}

/// True when encode size differs from the grabbed display size.
pub fn needs_scale(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> bool {
    src_w != dst_w || src_h != dst_h
}

/// Ordered capture graphs for `ddagrab` + the given encoder name.
/// First entries are lowest-latency / GPU-resident; last is the portable CPU path.
pub fn dda_capture_graphs(
    dxgi: Option<DxgiCapture>,
    fps: u32,
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    encoder: &str,
) -> Vec<String> {
    dda_capture_graphs_for(dxgi, fps, src_w, src_h, dst_w, dst_h, encoder, true)
}

/// FFmpeg `ddagrab` has no `allow_tearing` option; unknown keys make the
/// filter fail and we used to burn 3s per graph before falling back.
/// `draw_mouse=0` when the tablet paints a local overlay so the pointer is not
/// baked into the 50–100 ms video path.
pub fn dda_source_filter(output_idx: u32, fps: u32, draw_mouse: bool) -> String {
    let mouse = if draw_mouse { 1 } else { 0 };
    let dup = if crate::session_policy::ddagrab_duplicate_frames() {
        1
    } else {
        0
    };
    format!("ddagrab=output_idx={output_idx}:framerate={fps}:draw_mouse={mouse}:dup_frames={dup}")
}

pub fn dda_capture_graphs_for(
    dxgi: Option<DxgiCapture>,
    fps: u32,
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    encoder: &str,
    draw_mouse: bool,
) -> Vec<String> {
    // Indirect/virtual displays can be visible to GDI but absent from DXGI.
    // Never reinterpret their position in the monitor list as output zero.
    let Some(dxgi) = dxgi else {
        return Vec::new();
    };
    let grab = crate::session_policy::dda_poll_hz(fps);
    let dda = dda_source_filter(dxgi.output_index, grab, draw_mouse);
    let scale = needs_scale(src_w, src_h, dst_w, dst_h);
    dda_encoder_graphs(&dda, scale, dst_w, dst_h, encoder)
}

fn dda_encoder_graphs(dda: &str, scale: bool, dst_w: u32, dst_h: u32, encoder: &str) -> Vec<String> {
    let mut graphs = Vec::new();
    if encoder.contains("nvenc") {
        // ffmpeg vf_scale_d3d11.c hardcodes initial_pool_size = 10 and
        // always succeeds, so it must never be in the list or CUDA / CPU
        // never go live (ten pictures of glass vs GlideX native DDA).
        // Identity: ddagrab is already D3D11 BGRA and NVENC accepts D3D11.
        let cuda = if scale {
            format!("scale_cuda={dst_w}:{dst_h}:format=nv12")
        } else {
            "scale_cuda=format=nv12".to_string()
        };
        if !scale {
            // ddagrab hardcodes initial_pool_size = 8. hwmap mode=direct
            // only derives that same pool (extra_hw_frames is ignored).
            // reverse=1 is the path that allocates a new context:
            // initial_pool_size = 2 + extra_hw_frames. extra=0 → 2 surfaces,
            // matching Sunshine / GlideX native DDA. Raw dda stays as the
            // 8-pool fallback if reverse never emits IDR.
            graphs.push(format!("{dda},hwmap=reverse=1:extra_hw_frames=0"));
            graphs.push(dda.to_string());
        }
        // extra=0 only. Unkeyed hwmap keeps ffmpeg's 16-frame default.
        // mode=direct maps D3D11 textures; copy hwmap if that FATAL-rejects.
        // Encoder adds cuda@capture.
        graphs.push(format!(
            "{dda},hwmap=derive_device=cuda:mode=direct:extra_hw_frames=0,{cuda}:extra_hw_frames=0"
        ));
        graphs.push(format!(
            "{dda},hwmap=derive_device=cuda:extra_hw_frames=0,{cuda}:extra_hw_frames=0"
        ));
        // hwupload_cuda wants sysmem. A D3D11 frame that survives configure()
        // copies GPU→CPU→GPU every picture. Identity d3d11 wrap is the same
        // BGRA NVENC already rejected. CUDA miss goes to CPU bilinear.
    }
    if encoder.contains("qsv") {
        // ddagrab is D3D11. hwmap first avoids a sysmem upload (~1–2 ms).
        // Unkeyed hwmap still succeeds and keeps the default 16-frame pool.
        // Identity: scale_qsv with w/h is a VPP resize even at 1:1. Format
        // convert only, matching Sunshine ULL (no extra scale pass).
        let qsv = if scale {
            format!("scale_qsv=w={dst_w}:h={dst_h}:format=nv12")
        } else {
            "scale_qsv=format=nv12".to_string()
        };
        // extra=0 only. Encoder adds qsv@capture so this is not a 3s reject
        // into hwupload sysmem. mode=direct maps D3D11 textures on the
        // same Intel adapter; copy hwmap if that FATAL-rejects.
        // hwupload wants sysmem — a D3D11 graph that survived configure()
        // copies GPU→CPU→GPU every picture, same trap as hwupload_cuda.
        // Miss goes to CPU bilinear.
        graphs.push(format!(
            "{dda},hwmap=derive_device=qsv:mode=direct:extra_hw_frames=0,{qsv}:extra_hw_frames=0"
        ));
        graphs.push(format!(
            "{dda},hwmap=derive_device=qsv:extra_hw_frames=0,{qsv}:extra_hw_frames=0"
        ));
    }
    if encoder.contains("amf") {
        let vpp = if scale {
            format!("vpp_amf=w={dst_w}:h={dst_h}:format=nv12")
        } else {
            // Format convert only. w/h at 1:1 is still a VPP resize
            // (same trap as scale_qsv with w/h on identity).
            "vpp_amf=format=nv12".to_string()
        };
        if !scale {
            // Same 8-pool trap as NVENC identity: mode=direct derived
            // hwmap does not shrink ddagrab. reverse=1 extra=0 is a
            // 2-surface pool; raw dda stays as the 8-pool fallback.
            graphs.push(format!("{dda},hwmap=reverse=1:extra_hw_frames=0"));
            graphs.push(dda.to_string());
        }
        // extra=0 only. Encoder adds amf@capture. mode=direct maps D3D11
        // textures; copy hwmap if that FATAL-rejects. Without this GPU
        // NV12 convert, a BGRA reject used to skip straight to CPU
        // hwdownload (GPU→CPU every picture). scale_d3d11 must never
        // be in the list (hardcoded 10-frame pool).
        graphs.push(format!(
            "{dda},hwmap=derive_device=amf:mode=direct:extra_hw_frames=0,{vpp}:extra_hw_frames=0"
        ));
        graphs.push(format!(
            "{dda},hwmap=derive_device=amf:extra_hw_frames=0,{vpp}:extra_hw_frames=0"
        ));
        if !scale {
            graphs.push(format!(
                "{dda},hwmap=derive_device=d3d11:extra_hw_frames=0"
            ));
            graphs.push(format!("{dda},format=d3d11"));
        }
    }
    if scale {
        graphs.push(format!(
            "{dda},hwdownload,format=bgra,format=yuv420p,scale={dst_w}:{dst_h}:flags=bilinear"
        ));
    } else {
        graphs.push(format!("{dda},hwdownload,format=bgra,format=yuv420p"));
    }
    graphs
}

/// GDI uses signed virtual-desktop coordinates, including screens left/above primary.
pub fn gdigrab_input_args(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    fps: u32,
    draw_mouse: bool,
) -> [String; 14] {
    [
        "-f".into(),
        "gdigrab".into(),
        "-framerate".into(),
        fps.to_string(),
        "-offset_x".into(),
        x.to_string(),
        "-offset_y".into(),
        y.to_string(),
        "-video_size".into(),
        format!("{width}x{height}"),
        "-draw_mouse".into(),
        if draw_mouse { "1".into() } else { "0".into() },
        "-i".into(),
        "desktop".into(),
    ]
}

/// Software gdigrab scale filter; identity skips resampling.
pub fn gdigrab_vf(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> String {
    if needs_scale(src_w, src_h, dst_w, dst_h) {
        format!("format=yuv420p,scale={dst_w}:{dst_h}:flags=bilinear")
    } else {
        "format=yuv420p".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_skips_cpu_scale() {
        let graphs = dda_capture_graphs(Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0 }), 60, 1920, 1080, 1920, 1080, "libx264");
        assert_eq!(graphs.len(), 1);
        assert!(!graphs[0].contains("scale="));
        assert!(graphs[0].contains("yuv420p"));
    }

    #[test]
    fn cpu_scale_uses_bilinear_not_fast() {
        let graphs = dda_capture_graphs(Some(DxgiCapture { adapter_index: 0, output_index: 1, vendor_id: 0 }), 60, 2560, 1440, 1920, 1080, "libx264");
        let cpu = graphs.last().unwrap();
        assert!(cpu.contains("flags=bilinear"));
        assert!(!cpu.contains("fast_bilinear"));
    }

    #[test]
    fn nvenc_prefers_cuda_before_d3d11_pool() {
        let capture = DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x10DE };
        let graphs = dda_capture_graphs(Some(capture), 60, 2560, 1440, 1920, 1080, "h264_nvenc");
        assert!(graphs.len() >= 3);
        // Resize: scale_d3d11 always succeeds with a 10-frame GPU pool, so
        // it must not be in the list or CUDA / CPU never go live.
        assert!(graphs[0].contains("hwmap=derive_device=cuda:mode=direct:extra_hw_frames=0"));
        assert!(graphs[0].contains("scale_cuda=1920:1080:format=nv12"));
        assert!(!graphs[0].contains("scale_d3d11"));
        assert!(!graphs.iter().any(|g| g.contains("hwupload_cuda")));
        assert!(!graphs.iter().any(|g| g.contains("scale_d3d11")));
        let cuda_at = graphs.iter().position(|g| g.contains("scale_cuda")).unwrap();
        let cpu_at = graphs.iter().position(|g| g.contains("hwdownload")).unwrap();
        assert!(cuda_at < cpu_at);
        assert!(graphs.iter().any(|g| {
            g.contains("hwmap=derive_device=cuda:extra_hw_frames=0") && !g.contains("mode=direct")
        }));
        // Unkeyed CUDA hwmap is the 16-frame default pool — never emit it.
        assert!(!graphs.iter().any(|g| g.contains("derive_device=cuda") && !g.contains("extra_hw_frames")));
        assert_eq!(
            extra_hw_device_args(capture, &graphs[0]),
            ["-init_hw_device", "cuda=cuda@capture"]
        );
        assert!(extra_hw_device_args(capture, "ddagrab=output_idx=0").is_empty());
        assert!(graphs.last().unwrap().contains("hwdownload"));
        for g in &graphs {
            assert!(!g.contains("bitrate"));
            assert!(!g.contains("crf"));
        }
    }

    #[test]
    fn qsv_prefers_hwmap_before_upload() {
        let graphs = dda_capture_graphs(
            Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x8086 }),
            60, 1920, 1080, 1920, 1080, "h264_qsv",
        );
        assert!(graphs[0].contains("hwmap=derive_device=qsv:mode=direct:extra_hw_frames=0"));
        assert!(graphs[0].contains("scale_qsv=format=nv12:extra_hw_frames=0"));
        assert!(!graphs[0].contains("w=1920"));
        assert!(!graphs[0].contains("h=1080"));
        assert!(!graphs.iter().any(|g| g.contains("hwupload")));
        assert!(!graphs.iter().any(|g| g.contains("derive_device=qsv") && !g.contains("extra_hw_frames")));
        assert!(graphs.iter().any(|g| {
            g.contains("hwmap=derive_device=qsv:extra_hw_frames=0") && !g.contains("mode=direct")
        }));
        let cap = DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x8086 };
        assert_eq!(
            extra_hw_device_args(cap, &graphs[0]),
            ["-init_hw_device", "qsv=qsv@capture"]
        );
        assert!(extra_hw_device_args(cap, "ddagrab=output_idx=0").is_empty());
    }

    #[test]
    fn identity_nvenc_skips_d3d11_resize() {
        let same = dda_capture_graphs(
            Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x10DE }),
            60, 1920, 1080, 1920, 1080, "h264_nvenc",
        );
        // Identity: reverse hwmap is the only path that applies
        // extra_hw_frames (pool = 2 + extra). mode=direct derived does not.
        assert!(same[0].contains("hwmap=reverse=1:extra_hw_frames=0"));
        assert!(!same[0].contains("mode=direct"));
        assert!(!same[0].contains("scale_d3d11"));
        assert!(!same[0].contains("scale_cuda"));
        assert!(!same[0].contains("derive_device"));
        assert!(same.iter().any(|g| g.starts_with("ddagrab=") && !g.contains("hwmap")));
        assert!(extra_hw_device_args(
            DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x10DE },
            &same[0],
        ).is_empty());
        let cuda_at = same.iter().position(|g| g.contains("scale_cuda")).unwrap();
        let cpu_at = same.iter().position(|g| g.contains("hwdownload")).unwrap();
        assert!(cuda_at < cpu_at, "BGRA reject must try CUDA before CPU download");
        assert!(!same.iter().any(|g| g.contains("scale_d3d11")));
        assert!(same.iter().any(|g| g.contains("scale_cuda=format=nv12") && !g.contains("1920:1080")));
        assert!(!same.iter().any(|g| g.contains("derive_device=cuda") && !g.contains("extra_hw_frames")));
        let scaled = dda_capture_graphs(
            Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x10DE }),
            60, 2560, 1440, 1920, 1080, "h264_nvenc",
        );
        assert!(scaled[0].contains("hwmap=derive_device=cuda:mode=direct:extra_hw_frames=0"));
        assert!(scaled[0].contains("scale_cuda=1920:1080:format=nv12"));
        assert!(!scaled[0].contains("scale_d3d11"));
    }

    #[test]
    fn identity_qsv_skips_vpp_resize() {
        let scaled = dda_capture_graphs(
            Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x8086 }),
            60, 2560, 1440, 1920, 1080, "h264_qsv",
        );
        assert!(scaled[0].contains("scale_qsv=w=1920:h=1080:format=nv12:extra_hw_frames=0"));
    }

    #[test]
    fn amf_identity_stays_on_d3d11() {
        let graphs = dda_capture_graphs(
            Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x1002 }),
            60, 1920, 1080, 1920, 1080, "h264_amf",
        );
        // Identity: reverse hwmap is the only path that applies
        // extra_hw_frames (pool = 2 + extra). mode=direct derived does not.
        assert!(graphs[0].contains("hwmap=reverse=1:extra_hw_frames=0"));
        assert!(!graphs[0].contains("mode=direct"));
        assert!(!graphs[0].contains("derive_device"));
        assert!(!graphs[0].contains("hwupload"));
        assert!(!graphs[0].contains("hwdownload"));
        assert!(graphs.iter().any(|g| g.starts_with("ddagrab=") && !g.contains("hwmap")));
        assert!(graphs.iter().any(|g| g.contains("hwmap=derive_device=d3d11:extra_hw_frames=0")));
        assert!(!graphs.iter().any(|g| g.contains("hwmap=derive_device=d3d11") && !g.contains("extra_hw_frames")));
        assert!(graphs.iter().any(|g| g.contains("format=d3d11")));
        assert!(!graphs.iter().any(|g| g.contains("hwupload")));
        // BGRA reject must try GPU NV12 before CPU download (NVENC CUDA analog).
        assert!(graphs.iter().any(|g| {
            g.contains("vpp_amf=format=nv12") && !g.contains("w=") && !g.contains("h=")
        }));
        assert!(graphs.iter().any(|g| {
            g.contains("hwmap=derive_device=amf:mode=direct:extra_hw_frames=0")
        }));
        let amf_vpp = graphs.iter().position(|g| g.contains("vpp_amf")).unwrap();
        let cpu_at = graphs.iter().position(|g| g.contains("hwdownload")).unwrap();
        assert!(amf_vpp < cpu_at);
        let scaled = dda_capture_graphs(
            Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x1002 }),
            60, 2560, 1440, 1920, 1080, "h264_amf",
        );
        assert!(scaled[0].contains("hwmap=derive_device=amf:mode=direct:extra_hw_frames=0"));
        assert!(scaled[0].contains("vpp_amf=w=1920:h=1080:format=nv12"));
        assert!(scaled.iter().any(|g| {
            g.contains("hwmap=derive_device=amf:extra_hw_frames=0") && !g.contains("mode=direct")
        }));
        assert!(!scaled.iter().any(|g| g.contains("scale_d3d11")));
        let cap = DxgiCapture { adapter_index: 1, output_index: 0, vendor_id: 0x1002 };
        assert_eq!(
            extra_hw_device_args(cap, &scaled[0]),
            ["-init_hw_device", "amf=amf@capture"]
        );
    }

    #[test]
    fn gdigrab_identity_has_no_scale() {
        assert_eq!(gdigrab_vf(1280, 720, 1280, 720), "format=yuv420p");
        assert!(gdigrab_vf(1280, 720, 960, 540).contains("bilinear"));
    }

    #[test]
    fn defaults_do_not_force_downscale_helper() {
        assert!(!needs_scale(2560, 1440, 2560, 1440));
        assert!(needs_scale(2560, 1440, 1920, 1080));
    }

    #[test]
    fn second_adapter_keeps_output_slot_after_detached_outputs() {
        let capture = DxgiCapture {
            adapter_index: 1,
            output_index: 3,
            vendor_id: 0x10DE,
        };
        assert_eq!(
            capture.device_args(),
            ["-init_hw_device", "d3d11va=capture:1", "-filter_hw_device", "capture"]
        );
        let graphs = dda_capture_graphs(Some(capture), 60, 1920, 1080, 1920, 1080, "libx264");
        assert!(graphs.iter().all(|g| g.starts_with("ddagrab=output_idx=3:")));
    }

    #[test]
    fn virtual_dda_uses_real_ddagrab_options() {
        let filter = dda_source_filter(1, 60, true);
        assert!(filter.contains("output_idx=1"));
        assert!(filter.contains("draw_mouse=1"));
        assert!(filter.contains("dup_frames=0"));
        assert!(dda_source_filter(1, 60, false).contains("draw_mouse=0"));
        assert!(!filter.contains("allow_tearing"));
        let graphs = dda_capture_graphs_for(
            Some(DxgiCapture { adapter_index: 0, output_index: 1, vendor_id: 0 }),
            60, 1920, 1080, 1920, 1080, "h264_nvenc", true,
        );
        assert!(graphs.iter().all(|g| !g.contains("allow_tearing")));
        assert!(graphs.iter().all(|g| g.contains("output_idx=1")));
        assert!(graphs.iter().all(|g| g.contains("framerate=8000")));
        assert!(graphs.iter().all(|g| g.contains("dup_frames=0")));
    }

    #[test]
    fn gdi_only_virtual_monitor_never_tries_primary_duplication() {
        assert!(dda_capture_graphs(None, 60, 1280, 720, 1280, 720, "h264_nvenc").is_empty());
        assert_eq!(
            gdigrab_input_args(-1280, -720, 1280, 720, 60, true),
            [
                "-f", "gdigrab", "-framerate", "60", "-offset_x", "-1280",
                "-offset_y", "-720", "-video_size", "1280x720", "-draw_mouse",
                "1", "-i", "desktop",
            ]
        );
    }
}
