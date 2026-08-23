# Lighting 副屏协议 LIT1

主机（Windows / 日后 macOS）与 Android 客户端之间的 TCP 二进制协议。USB 模式下由电脑执行 `adb reverse tcp:17400 tcp:17400`，设备连接 `127.0.0.1:17400`。同一协议也可走局域网。

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
| 3 | Video | 电脑 → 设备 | 8 字节 PTS + Annex-B NAL（含起始码） |
| 4 | Touch | 设备 → 电脑 | 8 字节定长 |
| 5 | Heartbeat | 双向 | 空 |
| 6 | Error | 双向 | UTF-8 文本 |
| 7 | Audio | 电脑 → 设备 | 8 字节 PTS + PCM |

### Video 标志

- bit0：关键帧（IDR）
- bit1：编码器配置（AVC 的 SPS/PPS，或 HEVC 的 VPS/SPS/PPS）

### Video / Audio 的 PTS 前缀

`MSG_VIDEO` 与 `MSG_AUDIO` 的 payload 都以 **8 字节大端 PTS**（微秒，主机会话起点为 0）开头，后面才是媒体数据：

| 偏移 | 长度 | 含义 |
|------|------|------|
| 0 | 8 | `pts_us`，大端 `u64` |
| 8 | N-8 | Video：Annex-B；Audio：PCM |

旧实现若只发裸 NAL/PCM（长度 < 8 或无此前缀），设备侧会把整段当作媒体数据、PTS 视为 0。

Audio PCM：默认 48000 Hz、立体声、16-bit little-endian 交错。

### Hello JSON

```json
{
  "protocol": 1,
  "device": "Tablet Name",
  "screenWidth": 2560,
  "screenHeight": 1600,
  "maxFps": 120,
  "codecs": ["hevc", "avc"],
  "wantAudio": true,
  "decoderMaxWidth": 1920,
  "decoderMaxHeight": 1088,
  "decoderMaxFps": 60,
  "hwDecode": true,
  "alignment": 2,
  "soc": "qcom",
  "gsi": false,
  "brand": "google",
  "avcLimit": {
    "width": 1920,
    "height": 1088,
    "fps": 60,
    "hw": true,
    "name": "c2.qti.avc.decoder"
  },
  "hevcLimit": {
    "width": 1280,
    "height": 720,
    "fps": 30,
    "hw": true,
    "name": "c2.qti.hevc.decoder"
  }
}
```

`codecs` 按设备偏好排序。`avc` 为必须支持的底线。

能力字段均可缺省（主机按 0 / false / 空处理）：

| 字段 | 含义 |
|------|------|
| `wantAudio` | 设备希望收系统环回音频 |
| `decoderMaxWidth` / `decoderMaxHeight` / `decoderMaxFps` | 解码器封顶（软解或未分出 `*Limit` 时） |
| `hwDecode` | 当前偏好编解码是否硬解 |
| `alignment` | 宽高对齐（常见 2 或 16） |
| `soc` / `gsi` / `brand` | 芯片与 GSI 提示；GSI 上 HEVC 常不可靠 |
| `avcLimit` / `hevcLimit` | 该 codec 的硬解上限；缺省则回退到 `decoderMax*` |

### Config JSON

```json
{
  "width": 2560,
  "height": 1440,
  "fps": 60,
  "codec": "avc",
  "bitrateKbps": 40000,
  "audioEnabled": true,
  "audioSampleRate": 48000,
  "audioChannels": 2
}
```

`audioEnabled` 为 false 或缺省时设备不建 AudioTrack。`audioSampleRate` / `audioChannels` 缺省为 48000 / 2。

### Touch payload

| 偏移 | 长度 | 含义 |
|------|------|------|
| 0 | 1 | action：0 down / 1 move / 2 up / 3 cancel / 4 右键 down / 5 右键 up / 6 滚轮 / 7 横向滚轮 |
| 1 | 1 | pointerId |
| 2 | 2 | x，0–65535 映射画面宽（滚轮时为 notch×120 的有符号值） |
| 4 | 2 | y，0–65535 映射画面高 |
| 6 | 2 | pressure，0–65535 |

第一期主机主要处理 `pointerId == 0`，映射为所选显示器上的绝对鼠标。

## 会话顺序

1. 电脑监听 `0.0.0.0:17400`，USB 时再执行 `adb reverse`
2. 设备连接并发送 Hello
3. 电脑发送 Config，启动抓屏/编码
4. 先下发 `FLAG_CODEC_CONFIG` 的 Video，再下发帧（含 `FLAG_KEYFRAME` 的 IDR）
5. 设备回传 Touch；双方可发 Heartbeat（建议 2s）
6. 若开启音频，主机穿插 `MSG_AUDIO`

## 断线与编码器重启

- 用户点击「开始共享」后，主机保持同一 `TcpListener` 与（若已成功）`adb reverse`。客户端 EOF / broken pipe / 拔线只结束**当前** `handle_client`，然后回到「等待设备」，**不要**拆掉整次共享。只有用户点「停止」才解绑 reverse、结束会话。
- 共享仍在运行时，`accept` 与上一台客户端的拆除并行：ffmpeg `wait()` 和 `adb reverse` 刷新不得挡住下一台进来。设备侧第一次自动重连应略等（约 650ms + 抖动），避免打进拆除空窗；仍失败则本页点选手动重连。
- 编码器中途重启或切换（含 ddagrab → gdigrab）时，主机必须先再发一包 codec-config，再发一帧 IDR，然后才继续普通帧。设备在已配置解码器后若再收到 `FLAG_CODEC_CONFIG`，应重新 `configure` MediaCodec，避免黑屏。
- 设备停留在显示页且用户未返回时，socket 断开后应短回退自动重连数次；仍失败则留在本页，允许点状态栏/屏幕手动重连。
