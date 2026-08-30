//! Localhost JSON-RPC control plane for Electron (port 17401 by default).

use anyhow::{Context, Result};
use lighting_host::host_ipc::{
    self, encode_line, parse_request_line, RpcRequest, RpcResponse, SettingsPatchDto, DEFAULT_PORT,
    PORT_ENV,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::service::HostService;

pub fn resolve_port() -> u16 {
    std::env::var(PORT_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

pub async fn serve(service: HostService, port: u16) -> Result<()> {
    let addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind IPC {addr}"))?;
    tracing::info!("host IPC listening on {addr}");

    loop {
        let (socket, peer) = listener.accept().await?;
        tracing::info!("IPC client connected from {peer}");
        let svc = service.clone_handle();
        tokio::spawn(async move {
            if let Err(err) = handle_client(socket, svc).await {
                tracing::warn!("IPC client ended: {err:#}");
            }
        });
    }
}

async fn handle_client(socket: TcpStream, service: HostService) -> Result<()> {
    let (reader, mut writer) = socket.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = match parse_request_line(&line) {
            Ok(req) => dispatch(&service, req),
            Err(err) => RpcResponse {
                id: 0,
                ok: false,
                result: None,
                error: Some(format!("bad request: {err}")),
            },
        };
        let out = encode_line(&response)?;
        writer.write_all(out.as_bytes()).await?;
    }
    Ok(())
}

fn dispatch(service: &HostService, req: RpcRequest) -> RpcResponse {
    service.tick();
    let result = match req.method.as_str() {
        "ping" => Ok(serde_json::json!({ "pong": true })),
        "getState" => Ok(serde_json::to_value(service.state()).unwrap_or_default()),
        "refresh" => {
            service.refresh();
            Ok(serde_json::to_value(service.state()).unwrap_or_default())
        }
        "startShare" => match service.start_share() {
            Ok(()) => Ok(serde_json::to_value(service.state()).unwrap_or_default()),
            Err(err) => Err(err),
        },
        "stopShare" => {
            service.stop_share();
            Ok(serde_json::to_value(service.state()).unwrap_or_default())
        }
        "setSettings" => {
            match serde_json::from_value::<SettingsPatchDto>(req.params) {
                Ok(patch) => {
                    service.patch_settings(patch);
                    Ok(serde_json::to_value(service.state()).unwrap_or_default())
                }
                Err(err) => Err(format!("bad settings patch: {err}")),
            }
        }
        "installClient" => match service.install_client() {
            Ok(()) => Ok(serde_json::to_value(service.state()).unwrap_or_default()),
            Err(err) => Err(err),
        },
        other => Err(format!("unknown method: {other}")),
    };

    match result {
        Ok(value) => RpcResponse {
            id: req.id,
            ok: true,
            result: Some(value),
            error: None,
        },
        Err(error) => RpcResponse {
            id: req.id,
            ok: false,
            result: None,
            error: Some(error),
        },
    }
}

pub fn spawn_background(service: HostService, port: u16) {
    let rt = service.runtime();
    rt.spawn(async move {
        if let Err(err) = serve(service, port).await {
            tracing::error!("IPC server failed: {err:#}");
        }
    });
}

#[allow(dead_code)]
pub fn write_port_file(port: u16) {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let path = dir.join("lighting-ipc.port");
            let _ = std::fs::write(path, port.to_string());
        }
    }
    // Also expose via host_ipc constant for Electron discovery docs.
    let _ = host_ipc::DEFAULT_PORT;
}
