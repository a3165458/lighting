# Lighting 副屏 — 一键桌面版（Electron）

## 给新手：只要一个 exe

打包后得到：

- `Lighting副屏-0.1.0-便携版.exe` — **推荐**，双击即用，不用安装
- `Lighting副屏-0.1.0-安装包.exe` — 一键安装并创建桌面快捷方式

首次打开会自动下载：

- USB 调试工具（adb）
- 画面编码组件（ffmpeg）

保存到：`%APPDATA%\Lighting副屏\runtime\`

之后打开即可直接用。

## 开发者：本机打出便携版

```powershell
cd D:\Lighting
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
