# Host IPC（Electron ↔ lighting-host）

控制面协议，**不是** Android 用的 LIT1。

## 传输

- TCP `127.0.0.1:17401`
- 环境变量覆盖：`LIGHTING_IPC_PORT`
- 每个 Electron 进程生成独立的 `LIGHTING_IPC_TOKEN`（至少 32 字符），请求必须携带同一 token
- 单条 NDJSON 请求上限 64 KiB
- 编码：UTF-8，**一行一条 JSON**（NDJSON）

## 启动

```text
$env:LIGHTING_IPC_TOKEN = "至少 32 字符的随机值"
lighting-host.exe --ipc-only
```

Electron 会自动查找并拉起 `lighting-host.exe`（`extraResources` / 同目录 / `LIGHTING_HOST_PATH`）。
原生 egui 窗口模式不开放控制端口。

## 请求

```json
{"id":1,"method":"getState","params":{},"token":"..."}
```

方法：

| method | 说明 |
|--------|------|
| `ping` | 健康检查 |
| `getState` | 完整状态快照 |
| `refresh` | 刷新显示器 / adb 设备 |
| `startShare` | 开始共享 |
| `stopShare` | 停止共享 |
| `setSettings` | 部分更新设置（camelCase） |
| `installClient` | `adb install` 捆绑 APK |

## 响应

```json
{"id":1,"ok":true,"result":{...}}
{"id":1,"ok":false,"error":"..."}
```

`getState` / 变更类方法的 `result` 为 `HostStateDto`（见 `host-windows/src/host_ipc.rs`）。
