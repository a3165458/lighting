# Lighting

把 Android 平板或手机当成 Windows 电脑的扩展屏。走 USB 数据通道，不限 Type-C（USB-A、Micro-USB、转接线均可），目标 **2560×1440@60**。

当前仓库包含：

- `host-windows/`：Rust 主机（抓屏、硬编码、ADB reverse、触控注入）
- `android/`：Kotlin 客户端（MediaCodec 硬解）
- `protocol/PROTOCOL.md`：LIT1 二进制协议

macOS 主机为下一阶段。

## 电脑端（Windows）

依赖：

- [Rust](https://rustup.rs/)
- Visual Studio 2022 **Build Tools**（C++ 工作负载，提供 `link.exe`）
- [FFmpeg](https://www.gyan.dev/ffmpeg/builds/)（已加入 PATH）
- [Android Platform-Tools](https://developer.android.com/tools/releases/platform-tools) 中的 `adb`（USB 模式需要）

已编译的 Windows 主机：`host-windows/target/release/lighting-host.exe`。

主机二进制依赖 DXGI / WASAPI / `eframe`，只能在 Windows 上 `cargo run` / `cargo build`。Linux 上可跑不依赖 Win32 的辅助逻辑：

```bash
cd host-windows
cargo test --lib
```

启动主机：

```powershell
.\scripts\start.ps1
```

或在已配置 MSVC 的终端里：

```powershell
cd host-windows
cargo run --release
```

### 投屏模式说明

| 模式 | 作用 | 相当于 Win+P |
|------|------|--------------|
| 镜像主屏 | 与主屏同画面，**自动缩放到平板分辨率** | 复制 |
| 扩展屏（推荐） | 平板显示独立桌面 | 扩展 |
| 仅第二屏 | 桌面只出现在扩展屏/平板 | 仅第二屏幕 |

扩展 / 仅第二屏首次使用时，Lighting 会**自动准备虚拟显示器**（可能弹出一次管理员确认，类似 GlideX），无需自己去装驱动或敲命令。准备完成后：

- 在 Lighting 里选对应模式点「开始共享」即可
- 也可以用系统 **Win+P**：选「扩展」或「仅第二屏幕」——系统桌面会落到虚拟屏上，再由 Lighting 推到平板

镜像模式始终按平板物理分辨率编码，不会把 2K 主屏原样硬塞给非 2K 平板。

启动主机：

```powershell
cd host-windows
cargo run --release
```

或：

```powershell
.\scripts\start.ps1
```

## 平板 / 手机

1. 开启开发者选项和 **USB 调试**
2. 用数据线连电脑（必须是数据线，不能是仅充电线）
3. 用 Android Studio 打开 `android/` 并安装到设备，或直接安装已编译的 Debug APK：

```text
android/app/build/outputs/apk/debug/app-debug.apk
```

命令行编译 APK（需 JDK 17 + Android SDK）：

```powershell
$env:JAVA_HOME = "C:\Program Files\Microsoft\jdk-17.0.20.101-hotspot"
$env:ANDROID_HOME = "D:\Lighting\.runtime\android-sdk"
D:\Lighting\.runtime\gradle-8.9\bin\gradle.bat -p android assembleDebug
```
4. 电脑点「开始共享」（只插一台已授权设备时会自动选中）
5. 设备上打开 Lighting，点「USB 一键连接」

不需要填写地址或端口。局域网测试时，在电脑端点「高级设置」、在平板右上角点设置图标，填写电脑 IP。

连过一次之后，平板首页的「连接历史」可以一键重连同一台电脑。

只跑客户端的单元测试（不需要设备）：

```bash
gradle -p android testDebugUnitTest
```

## 第一期能力

- USB 有线（口型不限）+ 可选局域网 IP
- 扩展或镜像已有显示器
- H.264 硬编（NVENC / QSV / AMF / x264 回退），设备支持时可优先 HEVC
- 触摸映射为该显示器上的鼠标
- 2K60 为默认封顶，解码器不够时在主机侧把分辨率压到所选上限以内
