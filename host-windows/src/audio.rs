use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::time::Instant;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_SHAREMODE_SHARED, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
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
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).context("MMDeviceEnumerator")?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .context("GetDefaultAudioEndpoint")?;
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
                500_000,
                0,
                &fmt,
                None,
            )
            .context("IAudioClient.Initialize loopback")?;

        let capture: IAudioCaptureClient = client.GetService().context("IAudioCaptureClient")?;
        client.Start().context("IAudioClient.Start")?;
        tracing::info!("WASAPI loopback 48kHz stereo PCM16, 10ms packets");

        let mut acc = Vec::with_capacity(CHUNK_BYTES * 2);
        while !stop.load(Ordering::Relaxed) {
            let pending = capture.GetNextPacketSize().unwrap_or(0);
            if pending == 0 {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }
            let mut frames = 0u32;
            let mut flags = 0u32;
            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            if capture
                .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
                .is_err()
                || frames == 0
            {
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
                    // 网络堵住时丢掉最旧的一块，保住实时性
                    continue;
                }
            }
        }
        let _ = client.Stop();
    }
    Ok(())
}
