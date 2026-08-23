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

扩展屏（推荐）：

```powershell
winget install --id=VirtualDrivers.Virtual-Display-Driver -e
```

安装后在 **设置 → 系统 → 显示器** 中设为「扩展这些显示器」，分辨率可设 2560×1440。在 Lighting 里选择这块虚拟屏再点「开始共享」。

没有虚拟屏时也可以先选主屏做镜像，用来验证编码和 USB 通道。

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

不需要填写地址或端口。局域网测试请在两边的「高级」里填写电脑 IP。

## 第一期能力

- USB 有线（口型不限）+ 可选局域网 IP
- 扩展或镜像已有显示器
- H.264 硬编（NVENC / QSV / AMF / x264 回退），设备支持时可优先 HEVC
- 触摸映射为该显示器上的鼠标
- 2K60 为默认封顶，解码器不够时在主机侧把分辨率压到所选上限以内
