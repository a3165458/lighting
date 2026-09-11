# Lighting 副屏 — Windows 桌面版

## 给新手

到 [Releases](https://github.com/a3165458/lighting/releases/latest) 下载当前版本（v0.1.69）：

- `Lighting-0.1.69-setup.exe` — 安装包（推荐）
- `Lighting-0.1.69-portable.exe` — 便携版，双击即用

首次打开会自动准备 USB 调试工具（adb）和画面编码组件（ffmpeg），保存在 `%APPDATA%\Lighting副屏\runtime\`。

## 开发者：本机打包

```powershell
git pull
.\scripts\build-electron-win.ps1
```

产物在 `host-ui\release\`。

开发调试：

```powershell
.\scripts\build-windows.ps1
cd host-ui
npm install
npm run electron:dev
```

## 网页预览（无主机）

```bash
npm run dev
```
