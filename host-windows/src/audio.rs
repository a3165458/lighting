use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDevice, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};

const LOOPBACK: u32 = 0x0002_0000;
const AUTOCONVERT: u32 = 0x8000_0000;
const SRC_DEFAULT: u32 = 0x0800_0000;
const SILENT: u32 = 0x1;
const CHUNK_BYTES: usize = 48000 / 100 * 4; // 10ms stereo s16

#[derive(Clone)]
pub struct AudioPacket {
    pub pts_us: u64,
    pub pcm: Vec<u8>,
}

pub fn start_loopback(
    tx: SyncSender<AudioPacket>,
    stop: Arc<AtomicBool>,
    t0: Instant,
) -> Result<()> {
    std::thread::Builder::new()
        .name("lighting-audio".into())
        .spawn(move || {
            if let Err(err) = capture_loop(tx, stop, t0) {
                tracing::warn!("audio loopback ended: {err:#}");
            }
        })
        .context("spawn audio thread")?;
    Ok(())
}

fn capture_loop(tx: SyncSender<AudioPacket>, stop: Arc<AtomicBool>, t0: Instant) -> Result<()> {
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED)
            .ok()
            .context("CoInitializeEx")?;
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .context("MMDeviceEnumerator")?;

        while !stop.load(Ordering::Relaxed) {
            match capture_from_default(&enumerator, &tx, &stop, t0) {
                Ok(()) => {}
                Err(err) => tracing::warn!("audio loopback reopen: {err:#}"),
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(
                lighting_host::session_policy::audio_loopback_retry_ms(),
            ));
        }
    }
    Ok(())
}

fn capture_from_default(
    enumerator: &IMMDeviceEnumerator,
    tx: &SyncSender<AudioPacket>,
    stop: &Arc<AtomicBool>,
    t0: Instant,
) -> Result<()> {
    unsafe {
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .context("GetDefaultAudioEndpoint")?;
        let opened_id = device_id(&device);
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .context("Activate IAudioClient")?;

        let fmt = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 2,
            nSamplesPerSec: 48000,
            nAvgBytesPerSec: 48000 * 4,
            nBlockAlign: 4,
            wBitsPerSample: 16,
            cbSize: 0,
        };

        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                LOOPBACK | AUTOCONVERT | SRC_DEFAULT,
                lighting_host::session_policy::wasapi_buffer_hns(),
                0,
                &fmt,
                None,
            )
            .context("IAudioClient.Initialize loopback")?;

        let capture: IAudioCaptureClient = client.GetService().context("IAudioCaptureClient")?;
        client.Start().context("IAudioClient.Start")?;
        tracing::info!(
            "WASAPI loopback 48kHz stereo PCM16, 10ms packets (device {opened_id})"
        );

        let mut acc = Vec::with_capacity(CHUNK_BYTES * 2);
        let mut last_dev_check = Instant::now();
        let poll = Duration::from_millis(
            lighting_host::session_policy::audio_default_device_poll_ms(),
        );
        let follow = lighting_host::session_policy::audio_follow_default_render_device();

        while !stop.load(Ordering::Relaxed) {
            if follow && last_dev_check.elapsed() >= poll {
                last_dev_check = Instant::now();
                let now_id = default_render_id(enumerator);
                if now_id.as_ref() != Some(&opened_id) {
                    tracing::info!(
                        "default render device changed ({opened_id} -> {}); reopening loopback",
                        now_id.as_deref().unwrap_or("none")
                    );
                    let _ = client.Stop();
                    return Ok(());
                }
            }

            match capture.GetNextPacketSize() {
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Ok(_) => {}
                Err(err) => {
                    let hr = err.code().0 as u32;
                    tracing::warn!("WASAPI GetNextPacketSize hr=0x{hr:08x}; reopening loopback");
                    let _ = client.Stop();
                    return Ok(());
                }
            }

            let mut frames = 0u32;
            let mut flags = 0u32;
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            if let Err(err) = capture.GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
            {
                let hr = err.code().0 as u32;
                if lighting_host::session_policy::audio_hresult_is_device_lost(hr) {
                    tracing::warn!("WASAPI GetBuffer hr=0x{hr:08x}; reopening loopback");
                    let _ = client.Stop();
                    return Ok(());
                }
                continue;
            }
            if frames == 0 {
                continue;
            }
            let bytes = frames as usize * fmt.nBlockAlign as usize;
            if flags & SILENT != 0 {
                acc.resize(acc.len() + bytes, 0);
            } else if !data_ptr.is_null() {
                acc.extend_from_slice(std::slice::from_raw_parts(data_ptr, bytes));
            }
            let _ = capture.ReleaseBuffer(frames);

            while acc.len() >= CHUNK_BYTES {
                let pcm: Vec<u8> = acc.drain(..CHUNK_BYTES).collect();
                let pts_us = t0.elapsed().as_micros() as u64;
                if tx.try_send(AudioPacket { pts_us, pcm }).is_err() {
                    // Queue full: drop this chunk. The writer now drains up
                    // to audio_packets_per_video_frame so this is rare.
                    continue;
                }
            }
        }
        let _ = client.Stop();
    }
    Ok(())
}

fn default_render_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
    unsafe {
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        Some(device_id(&device))
    }
}

fn device_id(device: &IMMDevice) -> String {
    unsafe {
        match device.GetId() {
            Ok(pwstr) => {
                let s = pwstr_to_string(pwstr);
                CoTaskMemFree(Some(pwstr.0 as *const core::ffi::c_void));
                s
            }
            Err(_) => String::new(),
        }
    }
}

unsafe fn pwstr_to_string(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.0.add(len) != 0 {
        len += 1;
        if len > 4096 {
            break;
        }
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p.0, len))
}
