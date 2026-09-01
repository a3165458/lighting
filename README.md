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

| 模式 | 作用 |
|------|------|
| 镜像主屏 | 与主屏同画面，自动缩放到平板分辨率 |
| 扩展屏（推荐） | 虚拟扩展桌面；平板连接后按平板分辨率 1:1 输出 |
| 仅投扩展屏 | 只把扩展桌面投到平板（同样 1:1）；**不会**关掉电脑主屏 |

扩展相关模式会：

1. 自动激活 Virtual Display Driver 的虚拟显示器（named pipe `SETDISPLAYCOUNT`，不只装驱动）
2. 平板连上后，把虚拟屏改成平板物理分辨率再抓取，避免缩放浪费

说明：系统 Win+P「仅第二屏幕」会熄灭电脑主屏，Lighting 窗口也会一起没掉，因此产品内不采用该投影方式。

### SmartScreen / 360 提示「有病毒」？

未签名的 Windows 程序常被提示「发布者未知」或被 360 误报，**不等于真有木马**。  
临时处理：SmartScreen 点「仍要运行」；360 选允许/加入信任。  
长期方案见 [`docs/WINDOWS-SMARTSCREEN.md`](docs/WINDOWS-SMARTSCREEN.md)（代码签名证书）。

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
