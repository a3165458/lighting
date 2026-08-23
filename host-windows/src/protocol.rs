use anyhow::{bail, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PORT: u16 = 17400;
pub const MAGIC: &[u8; 4] = b"LIT1";
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

pub const MSG_HELLO: u8 = 1;
pub const MSG_CONFIG: u8 = 2;
pub const MSG_VIDEO: u8 = 3;
pub const MSG_TOUCH: u8 = 4;
pub const MSG_HEARTBEAT: u8 = 5;
pub const MSG_ERROR: u8 = 6;
pub const MSG_AUDIO: u8 = 7;

pub const FLAG_KEYFRAME: u8 = 1 << 0;
pub const FLAG_CODEC_CONFIG: u8 = 1 << 1;

#[derive(Debug, Clone)]
pub struct Message {
    pub ty: u8,
    pub flags: u8,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    pub protocol: u32,
    pub device: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub max_fps: u32,
    pub codecs: Vec<String>,
    #[serde(default)]
    pub want_audio: bool,
    #[serde(default)]
    pub decoder_max_width: u32,
    #[serde(default)]
    pub decoder_max_height: u32,
    #[serde(default)]
    pub decoder_max_fps: u32,
    #[serde(default)]
    pub hw_decode: bool,
    #[serde(default)]
    pub alignment: u32,
    #[serde(default)]
    pub soc: String,
    #[serde(default)]
    pub gsi: bool,
    #[serde(default)]
    pub brand: String,
    #[serde(default)]
    pub avc_limit: Option<CodecLimit>,
    #[serde(default)]
    pub hevc_limit: Option<CodecLimit>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodecLimit {
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub fps: u32,
    #[serde(default)]
    pub hw: bool,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub codec: String,
    pub bitrate_kbps: u32,
    #[serde(default)]
    pub audio_enabled: bool,
    #[serde(default = "default_audio_rate")]
    pub audio_sample_rate: u32,
    #[serde(default = "default_audio_channels")]
    pub audio_channels: u32,
    /// Shown in the tablet's connection history so the user recognizes the PC.
    #[serde(default)]
    pub host_name: String,
}

pub fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "这台电脑".into())
}

fn default_audio_rate() -> u32 {
    48000
}
fn default_audio_channels() -> u32 {
    2
}

#[derive(Debug, Clone, Copy)]
pub struct TouchEvent {
    pub action: u8,
    pub pointer_id: u8,
    pub x: u16,
    pub y: u16,
    pub pressure: u16,
}

impl TouchEvent {
    pub fn parse(payload: &[u8]) -> Result<Self> {
        if payload.len() < 8 {
            bail!("touch payload too short");
        }
        Ok(Self {
            action: payload[0],
            pointer_id: payload[1],
            x: u16::from_be_bytes([payload[2], payload[3]]),
            y: u16::from_be_bytes([payload[4], payload[5]]),
            pressure: u16::from_be_bytes([payload[6], payload[7]]),
        })
    }
}

pub async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Message> {
    let mut hdr = [0u8; 12];
    reader.read_exact(&mut hdr).await.context("read header")?;
    if &hdr[0..4] != MAGIC {
        bail!("bad magic, expected LIT1");
    }
    let ty = hdr[4];
    let flags = hdr[5];
    let len = u32::from_be_bytes(hdr[8..12].try_into().unwrap()) as usize;
    if len > MAX_PAYLOAD {
        bail!("payload too large: {len}");
    }
    let mut payload = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut payload).await.context("read payload")?;
    }
    Ok(Message { ty, flags, payload })
}

pub async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    ty: u8,
    flags: u8,
    payload: &[u8],
) -> Result<()> {
    if payload.len() > MAX_PAYLOAD {
        bail!("payload too large");
    }
    let mut hdr = [0u8; 12];
    hdr[0..4].copy_from_slice(MAGIC);
    hdr[4] = ty;
    hdr[5] = flags;
    hdr[8..12].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    writer.write_all(&hdr).await?;
    if !payload.is_empty() {
        writer.write_all(payload).await?;
    }
    writer.flush().await?;
    Ok(())
}

pub fn with_pts(pts_us: u64, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend_from_slice(&pts_us.to_be_bytes());
    out.extend_from_slice(data);
    out
}
