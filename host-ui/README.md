# Lighting 副屏 — Desktop UI (React + Electron)

## 网页预览

```bash
cd host-ui
npm install
npm run dev
```

## Electron 桌面壳（Windows exe）

开发（热更新）：

```bash
npm run electron:dev
```

本机打 Windows 包（需在 Windows 上执行，或用 CI）：

```powershell
cd D:\Lighting
.\scripts\build-electron-win.ps1
```

或：

```powershell
cd host-ui
npm install
npm run electron:build:win
```

产物在 `host-ui/release/`：

- `Lighting-*-portable.exe` — 绿色免安装
- `Lighting-*-win-x64.exe` — NSIS 安装包

也可以从 GitHub Actions 产物 `lighting-electron-windows` 下载。

> 说明：当前 Electron 壳包装的是 **UI 原型**（mock 交互）。真正抓屏 / ADB / 编码仍在 `host-windows` 的 `lighting-host.exe`。后续可把两边通过本地 IPC 打通。

## Design tokens

`src/styles/tokens.css` 是视觉单一来源；组件应使用 token，避免随意硬编码。
