//! Capture/scale graphs for FFmpeg Desktop Duplication.
//!
//! Prefer GPU-resident paths (no `hwdownload`) so we cut latency without
//! lowering bitrate/fps/resolution. CPU graphs stay as fallbacks and use
//! `bilinear` (not `fast_bilinear`) when scaling is required.

/// True when encode size differs from the grabbed display size.
pub fn needs_scale(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> bool {
    src_w != dst_w || src_h != dst_h
}

/// Ordered capture graphs for `ddagrab` + the given encoder name.
/// First entries are lowest-latency / GPU-resident; last is the portable CPU path.
pub fn dda_capture_graphs(
    dxgi_index: u32,
    fps: u32,
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    encoder: &str,
) -> Vec<String> {
    let dda = format!("ddagrab=output_idx={dxgi_index}:framerate={fps}:draw_mouse=1");
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
        let graphs = dda_capture_graphs(0, 60, 1920, 1080, 1920, 1080, "libx264");
        assert_eq!(graphs.len(), 1);
        assert!(!graphs[0].contains("scale="));
        assert!(graphs[0].contains("yuv420p"));
    }

    #[test]
    fn cpu_scale_uses_bilinear_not_fast() {
        let graphs = dda_capture_graphs(1, 60, 2560, 1440, 1920, 1080, "libx264");
        let cpu = graphs.last().unwrap();
        assert!(cpu.contains("flags=bilinear"));
        assert!(!cpu.contains("fast_bilinear"));
    }

    #[test]
    fn nvenc_prefers_cuda_before_cpu() {
        let graphs = dda_capture_graphs(0, 60, 2560, 1440, 1920, 1080, "h264_nvenc");
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
}
