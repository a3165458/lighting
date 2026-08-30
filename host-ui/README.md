# Lighting 副屏 — Desktop UI (React + Electron)

## 网页预览（无主机）

```bash
cd host-ui
npm install
npm run dev
```

浏览器模式下没有 `lightingHost` 桥，界面会显示未连接主机。

## Electron + 本地 IPC（推荐）

1. 先编 Windows 主机：

```powershell
.\scripts\build-windows.ps1
# -> host-windows\target\release\lighting-host.exe
```

2. 开 Electron 壳（会自动拉起 `--ipc-only` 主机）：

```powershell
cd host-ui
npm install
npm run electron:dev
```

3. 打安装包 / 便携包（会尝试打包同目录的 `lighting-host.exe`）：

```powershell
.\scripts\build-electron-win.ps1
```

产物：`host-ui\release\Lighting-*-portable.exe`

可选环境变量：

- `LIGHTING_HOST_PATH` — 指定主机 exe
- `LIGHTING_IPC_PORT` — 默认 `17401`

协议说明：`protocol/HOST_IPC.md`

## Design tokens

`src/styles/tokens.css` 是视觉单一来源。
