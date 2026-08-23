# Lighting 副屏协议 LIT1

主机（Windows / 日后 macOS）与 Android 客户端之间的 TCP 二进制协议。USB 模式下由电脑执行 `adb reverse tcp:17400 tcp:17400`，设备连接 `127.0.0.1:17400`。同一协议也可走局域网（第二期）。

默认端口：`17400`。字节序：大端。

## 帧格式

| 偏移 | 长度 | 含义 |
|------|------|------|
| 0 | 4 | 魔数 `LIT1`（`0x4C495431`） |
| 4 | 1 | 消息类型 |
| 5 | 1 | 标志 |
| 6 | 2 | 保留，填 0 |
| 8 | 4 | payload 长度 |
| 12 | N | payload |

单帧 payload 上限 16 MiB。

## 消息类型

| 值 | 名称 | 方向 | payload |
|----|------|------|---------|
| 1 | Hello | 设备 → 电脑 | UTF-8 JSON |
| 2 | Config | 电脑 → 设备 | UTF-8 JSON |
| 3 | Video | 电脑 → 设备 | Annex-B NAL（含起始码） |
| 4 | Touch | 设备 → 电脑 | 8 字节定长 |
| 5 | Heartbeat | 双向 | 空 |
| 6 | Error | 双向 | UTF-8 文本 |

### Video 标志

- bit0：关键帧（IDR）
- bit1：编码器配置（AVC 的 SPS/PPS，或 HEVC 的 VPS/SPS/PPS）

### Hello JSON

```json
{
  "protocol": 1,
  "device": "Tablet Name",
  "screenWidth": 2560,
  "screenHeight": 1600,
  "maxFps": 120,
  "codecs": ["hevc", "avc"]
}
```

`codecs` 按设备偏好排序。`avc` 为必须支持的底线。

### Config JSON

```json
{
  "width": 2560,
  "height": 1440,
  "fps": 60,
  "codec": "avc",
  "bitrateKbps": 40000
}
```

### Touch payload

| 偏移 | 长度 | 含义 |
|------|------|------|
| 0 | 1 | action：0 down / 1 move / 2 up / 3 cancel |
| 1 | 1 | pointerId |
| 2 | 2 | x，0–65535 映射画面宽 |
| 4 | 2 | y，0–65535 映射画面高 |
| 6 | 2 | pressure，0–65535 |

第一期主机只处理 `pointerId == 0`，映射为所选显示器上的绝对鼠标。

## 会话顺序

1. 电脑监听 `0.0.0.0:17400`，USB 时再执行 `adb reverse`
2. 设备连接并发送 Hello
3. 电脑发送 Config，启动抓屏/编码
4. 先下发 codec config 的 Video，再下发帧
5. 设备回传 Touch；双方可发 Heartbeat（建议 2s）
