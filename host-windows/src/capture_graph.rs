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

fn hw_frame_pool_sizes() -> Vec<u32> {
    let a = crate::session_policy::hw_extra_frames();
    let b = crate::session_policy::hw_extra_frames_fallback();
    if a == b {
        vec![a]
    } else {
        vec![a, b]
    }
}

fn dda_encoder_graphs(dda: &str, scale: bool, dst_w: u32, dst_h: u32, encoder: &str) -> Vec<String> {
    let mut graphs = Vec::new();
    let extras = hw_frame_pool_sizes();
    if encoder.contains("nvenc") {
        // GlideX / Sunshine: ddagrab is already D3D11. scale_d3d11 converts
        // BGRA→NV12 on the same device and NVENC consumes D3D11 frames.
        // Try extra_hw_frames=1 first. An unkeyed scale_d3d11 still succeeds
        // on new ffmpeg and then uses the filter default pool (often 16
        // pictures). Unknown extra_hw_frames on old ffmpeg fails this graph
        // (~3s); the unkeyed graph below is that fallback.
        for extra in &extras {
            graphs.push(format!(
                "{dda},scale_d3d11=width={dst_w}:height={dst_h}:format=nv12:extra_hw_frames={extra}"
            ));
        }
        graphs.push(format!(
            "{dda},scale_d3d11=width={dst_w}:height={dst_h}:format=nv12"
        ));
        for extra in &extras {
            if scale {
                graphs.push(format!(
                    "{dda},hwupload_cuda=extra_hw_frames={extra},scale_cuda={dst_w}:{dst_h}:format=nv12"
                ));
            } else {
                graphs.push(format!(
                    "{dda},hwupload_cuda=extra_hw_frames={extra},scale_cuda=format=nv12"
                ));
            }
        }
        graphs.push(format!(
            "{dda},hwmap=derive_device=cuda:mode=direct,scale_cuda={dst_w}:{dst_h}:format=nv12"
        ));
    }
    if encoder.contains("qsv") {
        // ddagrab is D3D11. hwmap first avoids a sysmem upload (~1–2 ms).
        // Same trap as NVENC: unkeyed hwmap still succeeds and keeps the
        // default 16-frame pool. Try extra_hw_frames=1 first.
        for extra in &extras {
            graphs.push(format!(
                "{dda},hwmap=derive_device=qsv:extra_hw_frames={extra},scale_qsv=w={dst_w}:h={dst_h}:format=nv12:extra_hw_frames={extra}"
            ));
        }
        graphs.push(format!(
            "{dda},hwmap=derive_device=qsv,scale_qsv=w={dst_w}:h={dst_h}:format=nv12"
        ));
        for extra in &extras {
            graphs.push(format!(
                "{dda},hwupload=extra_hw_frames={extra},hwmap=derive_device=qsv,scale_qsv=w={dst_w}:h={dst_h}:format=nv12:extra_hw_frames={extra}"
            ));
        }
    }
    if encoder.contains("amf") {
        if scale {
            for extra in &extras {
                graphs.push(format!(
                    "{dda},scale_d3d11=width={dst_w}:height={dst_h}:format=nv12:extra_hw_frames={extra}"
                ));
            }
            graphs.push(format!(
                "{dda},scale_d3d11=width={dst_w}:height={dst_h}:format=nv12"
            ));
            for extra in &extras {
                graphs.push(format!(
                    "{dda},hwupload=extra_hw_frames={extra},scale_d3d11={dst_w}:{dst_h}:format=nv12"
                ));
            }
        } else {
            // Stay on D3D11. hwmap first; hwupload of an identity frame is a copy.
            for extra in &extras {
                graphs.push(format!(
                    "{dda},hwmap=derive_device=d3d11:extra_hw_frames={extra}"
                ));
            }
            graphs.push(format!("{dda},hwmap=derive_device=d3d11"));
            graphs.push(format!("{dda},format=d3d11"));
            for extra in &extras {
                graphs.push(format!(
                    "{dda},hwupload=extra_hw_frames={extra},format=d3d11"
                ));
            }
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
    fn nvenc_prefers_d3d11_before_cuda() {
        let graphs = dda_capture_graphs(Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0 }), 60, 2560, 1440, 1920, 1080, "h264_nvenc");
        assert!(graphs.len() >= 4);
        assert!(graphs[0].contains("scale_d3d11"));
        assert!(graphs[0].contains("format=nv12"));
        assert!(graphs[0].contains("extra_hw_frames=1"));
        assert!(!graphs[0].contains("hwupload_cuda"));
        assert!(graphs.iter().any(|g| g.contains("scale_d3d11") && !g.contains("extra_hw_frames")));
        assert!(graphs.iter().any(|g| g.contains("hwupload_cuda") && g.contains("scale_cuda")));
        assert!(graphs.iter().any(|g| g.contains("extra_hw_frames=2")));
        assert!(graphs.last().unwrap().contains("hwdownload"));
        // Same bitrate path — graphs must not embed bitrate/fps quality knobs.
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
        assert!(graphs[0].contains("hwmap=derive_device=qsv:extra_hw_frames=1"));
        assert!(graphs[0].contains("scale_qsv=w=1920:h=1080:format=nv12:extra_hw_frames=1"));
        assert!(!graphs[0].contains("hwupload"));
        assert!(graphs.iter().any(|g| g.contains("hwmap=derive_device=qsv,") && !g.contains("extra_hw_frames")));
        assert!(graphs.iter().any(|g| g.contains("hwupload")));
    }

    #[test]
    fn amf_identity_stays_on_d3d11() {
        let graphs = dda_capture_graphs(
            Some(DxgiCapture { adapter_index: 0, output_index: 0, vendor_id: 0x1002 }),
            60, 1920, 1080, 1920, 1080, "h264_amf",
        );
        assert!(graphs[0].contains("hwmap=derive_device=d3d11:extra_hw_frames=1"));
        assert!(!graphs[0].contains("hwupload"));
        assert!(!graphs[0].contains("hwdownload"));
        assert!(graphs.iter().any(|g| g.contains("hwmap=derive_device=d3d11") && !g.contains("extra_hw_frames")));
        assert!(graphs.iter().any(|g| g.contains("format=d3d11")));
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
