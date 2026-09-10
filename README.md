# Lighting 副屏

把 Android 平板或手机当成 Windows 电脑的第二块屏幕。走 USB 数据线（Type-C、USB-A、Micro-USB、转接线均可），不依赖 Wi‑Fi。

**当前版本：[v0.1.67](https://github.com/a3165458/lighting/releases/tag/v0.1.67)**

## 下载

到 [Releases](https://github.com/a3165458/lighting/releases/latest) 取最新包：

| 文件 | 说明 |
|------|------|
| `Lighting-0.1.67-setup.exe` | 安装包（推荐） |
| `Lighting-0.1.67-portable.exe` | 便携版，不用安装 |
| `Lighting.apk` | 平板客户端；也可在电脑里点「重新安装」推送 |

未签名的 Windows 程序可能被 SmartScreen / 360 提示「发布者未知」，不等于有病毒。点「更多信息 → 仍要运行」即可。说明见 [`docs/WINDOWS-SMARTSCREEN.md`](docs/WINDOWS-SMARTSCREEN.md)。

## 使用

1. 托盘里把旧 Lighting 退干净
2. 安装 `Lighting-0.1.67-setup.exe`，或打开便携包
3. 平板开启 **开发者选项** 和 **USB 调试**，用数据线连电脑（必须是数据线）
4. 电脑点 **开始共享**
5. 设置里点 **重新安装**，覆盖平板上的客户端
6. 平板打开 Lighting，点 **USB 一键连接**

不需要填写地址或端口。主机默认只监听 USB 回环，不会把桌面流暴露到局域网。

连过一次之后，平板首页的「连接历史」可以一键重连同一台电脑。

## 三种模式

| 模式 | 作用 |
|------|------|
| 镜像主屏 | 与电脑主屏同画面，按平板分辨率编码 |
| 双屏扩展 | 平板作为独立桌面，电脑屏继续亮 |
| 仅平板 | 虚拟屏 1:1 铺满平板，并关掉电脑屏。平板休眠或断开后会把电脑屏亮回来 |

**锁屏无法投屏**是 Windows 限制：抓屏抓不到安全桌面。请关掉自动锁屏，投屏时不要按 Win+L。合盖请在电源设置里设为不采取任何操作。

## 当前能力（v0.1.67）

- USB 有线投屏；可选局域网（高级设置里把监听改为 `0.0.0.0`，仅建议在可信网络使用）
- 镜像 / 扩展 / 仅平板
- H.264 硬编（NVENC / QSV / AMF，不够则 x264），设备支持时可走 HEVC
- 画面里走 Windows 系统光标
- 系统声音环回；默认播放设备切换（例如关掉蓝牙音响）后会跟过去
- 虚拟屏尽量贴近平板分辨率，能铺满就铺满
- 仅平板休眠或断开后，强制恢复电脑主屏（不再依赖 Ctrl+Win+Shift+B）
- 触摸映射为该显示器上的鼠标

## 仓库结构

- `host-windows/`：Windows 主机（抓屏、编码、ADB reverse、触控）
- `host-ui/`：Windows 一键桌面版（Electron）
- `android/`：平板客户端
- `protocol/PROTOCOL.md`：LIT1 协议

macOS 主机尚未提供。

开发者在 Windows 上从源码编译，见 [`host-ui/README.md`](host-ui/README.md)。
