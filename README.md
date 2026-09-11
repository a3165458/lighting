# Lighting 副屏

把 Android 平板或手机当成 Windows 电脑的第二块屏幕。走 USB 数据线（Type-C、USB-A、Micro-USB、转接线均可），不依赖 Wi‑Fi。

**当前版本：[v0.1.70](https://github.com/a3165458/lighting/releases/tag/v0.1.70)**（修复仅平板模式下休眠、断线后的电脑屏恢复流程）

## 下载

到 [Releases](https://github.com/a3165458/lighting/releases/latest) 取最新包：

| 文件 | 说明 |
|------|------|
| `Lighting-0.1.70-setup.exe` | 安装包（推荐） |
| `Lighting-0.1.70-portable.exe` | 便携版，不用安装 |
| `Lighting.apk` | 平板客户端；也可在电脑里点「重新安装」推送 |

未签名的 Windows 程序可能被 SmartScreen / **360 报毒**，不等于真有木马。临时：360 选「允许 / 加入信任区」。长期方案见 [`docs/WINDOWS-SMARTSCREEN.md`](docs/WINDOWS-SMARTSCREEN.md) 和 [Code signing policy](docs/CODE-SIGNING.md)。

## 使用

1. 托盘里把旧 Lighting 退干净
2. 安装 `Lighting-0.1.70-setup.exe`，或打开便携包
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

### v0.1.70：仅平板休眠恢复修复

- 关闭电脑屏前保存实际显示路径、分辨率和桌面位置；仅平板输出不写入 Windows 的持久显示配置。
- 平板断开、停止共享或编码出错后，先终止编码器，再恢复关闭电脑屏前的显示配置，保留虚拟扩展屏。不使用“强制仅电脑屏幕”的恢复方案。
- 电脑休眠/唤醒通知会结束当前共享并触发恢复；唤醒后需重新点 **开始共享**。仅平板自身休眠或断开时，主机继续等待连接。
- 平板停止响应心跳或 TCP 发送阻塞时会中断旧连接，避免恢复电脑屏一直等待发送线程。

验证边界：已通过自动化测试及构建检查，尚未在受影响的 Windows 实机上完成显卡休眠唤醒验证。升级后请验证“平板操作电脑休眠 → 唤醒”“平板熄屏/拔线”和再次开始共享，确认电脑屏恢复且虚拟扩展屏仍可用。

## 当前能力（v0.1.70）

- USB 有线投屏；可选局域网（高级设置里把监听改为 `0.0.0.0`，仅建议在可信网络使用）
- 镜像 / 扩展 / 仅平板
- H.264 硬编（NVENC / QSV / AMF，不够则 x264），设备支持时可走 HEVC
- 画面里走 Windows 系统光标
- 系统声音环回；默认播放设备切换（例如关掉蓝牙音响）后会跟过去
- 虚拟屏尽量贴近平板分辨率，能铺满就铺满
- 仅平板休眠或断开后，在编码器退出后恢复电脑屏和虚拟扩展屏配置
- 触摸映射为该显示器上的鼠标

## 仓库结构

- `host-windows/`：Windows 主机（抓屏、编码、ADB reverse、触控）
- `host-ui/`：Windows 一键桌面版（Electron）
- `android/`：平板客户端
- `protocol/PROTOCOL.md`：LIT1 协议

macOS 主机尚未提供。

开发者在 Windows 上从源码编译，见 [`host-ui/README.md`](host-ui/README.md)。

## Code signing policy

Windows 安装包计划通过 [SignPath Foundation](https://signpath.org/) 免费签名。政策、角色和隐私说明见 [`docs/CODE-SIGNING.md`](docs/CODE-SIGNING.md)。

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).
