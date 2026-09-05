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
    // Indirect/virtual displays can be visible to GDI but absent from DXGI.
    // Never reinterpret their position in the monitor list as output zero.
    let Some(dxgi) = dxgi else {
        return Vec::new();
    };
    let dda = format!(
        "ddagrab=output_idx={}:framerate={fps}:draw_mouse=1",
        dxgi.output_index
    );
    let scale = needs_scale(src_w, src_h, dst_w, dst_h);
    let mut graphs = Vec::new();

    if encoder.contains("nvenc") {
        if scale {
            graphs.push(format!(
                "{dda},hwmap=derive_device=cuda:mode=direct,scale_cuda={dst_w}:{dst_h}:format=nv12"
            ));
            graphs.push(format!(
                "{dda},hwupload_cuda,scale_cuda={dst_w}:{dst_h}:format=nv12"
            ));
        } else {
            // Identity size: still run scale_cuda to convert to NV12 on GPU.
            graphs.push(format!(
                "{dda},hwmap=derive_device=cuda:mode=direct,scale_cuda={dst_w}:{dst_h}:format=nv12"
            ));
            graphs.push(format!("{dda},hwupload_cuda,scale_cuda=format=nv12"));
        }
    }

    if encoder.contains("qsv") {
        if scale {
            graphs.push(format!(
                "{dda},hwmap=derive_device=qsv,scale_qsv=w={dst_w}:h={dst_h}:format=nv12"
            ));
        } else {
            graphs.push(format!(
                "{dda},hwmap=derive_device=qsv,scale_qsv=w={dst_w}:h={dst_h}:format=nv12"
            ));
        }
    }

    if encoder.contains("amf") {
        if scale {
            graphs.push(format!(
                "{dda},hwupload=extra_hw_frames=2,scale_d3d11={dst_w}:{dst_h}:format=nv12"
            ));
        } else {
            graphs.push(format!(
                "{dda},hwupload=extra_hw_frames=2,format=d3d11,hwdownload,format=nv12"
            ));
        }
    }

    // Portable CPU fallback: skip scale on identity to preserve every pixel.
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
pub fn gdigrab_input_args(x: i32, y: i32, width: u32, height: u32, fps: u32) -> [String; 14] {
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
        "1".into(),
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
        let graphs = dda_capture_graphs(Some(DxgiCapture { adapter_index: 0, output_index: 0 }), 60, 1920, 1080, 1920, 1080, "libx264");
        assert_eq!(graphs.len(), 1);
        assert!(!graphs[0].contains("scale="));
        assert!(graphs[0].contains("yuv420p"));
    }

    #[test]
    fn cpu_scale_uses_bilinear_not_fast() {
        let graphs = dda_capture_graphs(Some(DxgiCapture { adapter_index: 0, output_index: 1 }), 60, 2560, 1440, 1920, 1080, "libx264");
        let cpu = graphs.last().unwrap();
        assert!(cpu.contains("flags=bilinear"));
        assert!(!cpu.contains("fast_bilinear"));
    }

    #[test]
    fn nvenc_prefers_cuda_before_cpu() {
        let graphs = dda_capture_graphs(Some(DxgiCapture { adapter_index: 0, output_index: 0 }), 60, 2560, 1440, 1920, 1080, "h264_nvenc");
        assert!(graphs.len() >= 3);
        assert!(graphs[0].contains("scale_cuda"));
        assert!(graphs.last().unwrap().contains("hwdownload"));
        // Same bitrate path — graphs must not embed bitrate/fps quality knobs.
        for g in &graphs {
            assert!(!g.contains("bitrate"));
            assert!(!g.contains("crf"));
        }
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
        };
        assert_eq!(
            capture.device_args(),
            ["-init_hw_device", "d3d11va=capture:1", "-filter_hw_device", "capture"]
        );
        let graphs = dda_capture_graphs(Some(capture), 60, 1920, 1080, 1920, 1080, "libx264");
        assert!(graphs.iter().all(|g| g.starts_with("ddagrab=output_idx=3:")));
    }

    #[test]
    fn gdi_only_virtual_monitor_never_tries_primary_duplication() {
        assert!(dda_capture_graphs(None, 60, 1280, 720, 1280, 720, "h264_nvenc").is_empty());
        assert_eq!(
            gdigrab_input_args(-1280, -720, 1280, 720, 60),
            [
                "-f", "gdigrab", "-framerate", "60", "-offset_x", "-1280",
                "-offset_y", "-720", "-video_size", "1280x720", "-draw_mouse",
                "1", "-i", "desktop",
            ]
        );
    }
}
