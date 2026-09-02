//! Host ↔ Electron control-plane messages (not LIT1).
//! Wire format: newline-delimited JSON over TCP `127.0.0.1:17401`.

use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 17401;
pub const PORT_ENV: &str = "LIGHTING_IPC_PORT";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HostSettingsDto {
    pub selected_display: usize,
    pub selected_device: usize,
    pub share_mode: String,
    pub quality_pct: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub send_audio: bool,
    pub prefer_hevc: bool,
    pub res_cap: String,
    pub touch_relay: bool,
    pub keyboard_relay: bool,
    pub bind_host: String,
    pub bind_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HostStateDto {
    pub connected: bool,
    pub sharing: bool,
    pub phase: String,
    pub detail: String,
    pub transport: String,
    pub client_name: String,
    pub client_addr: String,
    pub codec: String,
    pub frames: u64,
    pub bitrate_kbps: u32,
    pub latency_ms: u32,
    pub loss_permille: u32,
    pub bytes_sent: u64,
    pub connected_secs: u64,
    pub usb_hint: String,
    pub usb_tone: String,
    pub device_detected: bool,
    pub client_app_missing: bool,
    pub client_app_version: String,
    pub can_install_apk: bool,
    pub install_inflight: bool,
    pub multi_device: bool,
    pub displays: Vec<DisplayDto>,
    pub devices: Vec<DeviceDto>,
    pub settings: HostSettingsDto,
    pub last_error: String,
    pub host_version: String,
    #[serde(default)]
    pub activity_title: String,
    #[serde(default)]
    pub activity_detail: String,
    #[serde(default)]
    pub activity_steps: Vec<ActivityStepDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStepDto {
    pub id: String,
    pub label: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayDto {
    pub id: String,
    pub label: String,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub virtual_display: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub id: String,
    pub label: String,
    pub serial: String,
    pub state: String,
    pub client_installed: Option<bool>,
    pub client_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatchDto {
    pub selected_display: Option<usize>,
    pub selected_device: Option<usize>,
    pub share_mode: Option<String>,
    pub quality_pct: Option<u32>,
    pub fps: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub send_audio: Option<bool>,
    pub prefer_hevc: Option<bool>,
    pub res_cap: Option<String>,
    pub touch_relay: Option<bool>,
    pub keyboard_relay: Option<bool>,
    pub bind_host: Option<String>,
    pub bind_port: Option<u16>,
}

pub fn encode_line(value: &impl Serialize) -> Result<String, serde_json::Error> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}

pub fn parse_request_line(line: &str) -> Result<RpcRequest, serde_json::Error> {
    serde_json::from_str(line.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_request() {
        let req = RpcRequest {
            id: 7,
            method: "setSettings".into(),
            params: serde_json::json!({ "qualityPct": 80, "fps": 60 }),
        };
        let line = encode_line(&req).unwrap();
        let parsed = parse_request_line(&line).unwrap();
        assert_eq!(parsed.id, 7);
        assert_eq!(parsed.method, "setSettings");
        assert_eq!(parsed.params["qualityPct"], 80);
    }

    #[test]
    fn state_dto_uses_camel_case() {
        let state = HostStateDto {
            sharing: true,
            usb_hint: "ok".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"usbHint\""));
        assert!(json.contains("\"sharing\":true"));
    }
}
