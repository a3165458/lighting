# Lighting 副屏

把 Android 平板或手机当成 Windows 电脑的第二块屏幕。走 USB 数据线（Type-C、USB-A、Micro-USB、转接线均可），不依赖 Wi‑Fi。

**当前版本：[v0.1.73](https://github.com/a3165458/lighting/releases/tag/v0.1.73)**（修复仅平板模式刷新率回退导致的延迟和音频卡顿）

## 下载

到 [Releases](https://github.com/a3165458/lighting/releases/latest) 取最新包：

| 文件 | 说明 |
|------|------|
| `Lighting-0.1.73-setup.exe` | 安装包（推荐） |
| `Lighting-0.1.73-portable.exe` | 便携版，不用安装 |
| `Lighting.apk` | 平板客户端；也可在电脑里点「重新安装」推送 |

未签名的 Windows 程序可能被 SmartScreen / **360 报毒**，不等于真有木马。临时：360 选「允许 / 加入信任区」。长期方案见 [`docs/WINDOWS-SMARTSCREEN.md`](docs/WINDOWS-SMARTSCREEN.md) 和 [Code signing policy](docs/CODE-SIGNING.md)。

## 使用

1. 托盘里把旧 Lighting 退干净
2. 安装 `Lighting-0.1.73-setup.exe`，或打开便携包
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

### v0.1.72：仅平板关断与双屏扩展都不会只剩主屏

- 仅平板关断和正常扩展结束时走同一套恢复：虚拟屏丢了会再做 Win+P 扩展。
- 会话异常退出时也走这套恢复，不再只重申电脑主屏。
- 虚拟共享不会写成“仅电脑屏幕”或“仅第二屏幕”的持久配置。

验证边界：已通过本地 147 项主机测试。尚未在受影响电脑上验证休眠唤醒与结束共享后的双屏布局。升级后请确认显示器列表同时有主屏和虚拟屏。

### v0.1.73：修复仅平板模式延迟和音频卡顿

- 修复显示拓扑切换后虚拟屏刷新率可能回退到 30Hz 的问题。
- 切换完成后重新应用平板分辨率和目标刷新率，再启动抓屏编码。
- 针对 60Hz 的联想小新 Pad 2020，避免视频回压连带造成声音卡顿。

### v0.1.71：仅平板恢复后仍保留虚拟屏

- 关闭电脑屏前的显示快照会回放，随后强制 Win+P 扩展，避免只剩主屏。
- 虚拟屏即使被 Windows 当成主屏，列表仍显示「虚拟屏」，不会被标成电脑主屏。
- 不使用 0.1.67/0.1.68 的“仅电脑屏幕”方案。

### v0.1.70：仅平板休眠恢复

- 关闭电脑屏前保存实际显示路径；电脑休眠会结束共享，唤醒后需重新开始。

## 当前能力（v0.1.73）

- USB 有线投屏；可选局域网（高级设置里把监听改为 `0.0.0.0`，仅建议在可信网络使用）
- 镜像 / 扩展 / 仅平板
- H.264 硬编（NVENC / QSV / AMF，不够则 x264），设备支持时可走 HEVC
- 画面里走 Windows 系统光标
- 系统声音环回；默认播放设备切换（例如关掉蓝牙音响）后会跟过去
- 虚拟屏尽量贴近平板分辨率，能铺满就铺满
- 仅平板关断或双屏扩展结束后，虚拟屏丢失时会再扩展，避免只剩主屏
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
