# Windows SmartScreen / 杀软误报说明

## 这不是「真有病毒」

`Lighting-*-portable.exe` / 安装包若显示：

- **Windows 已保护你的电脑**（发布者未知）
- 360 / Defender 提示「危险 / 木马」

在 **未做代码签名（Authenticode）** 时非常常见，尤其是：

- 新发布的小众开源软件（下载量低，无信誉）
- 会安装/启用 **虚拟显示驱动**（IddCx）的程序（行为像驱动工具，易被启发式拦截）

当前 Release 构建默认 **未签名**（`发布者未知`），所以会出现你看到的 SmartScreen 对话框。

---

## 你现在可以怎么用（临时）

### Microsoft Defender SmartScreen

1. 点 **「更多信息」**（有的系统默认只显示「不运行」）
2. 再点 **「仍要运行」**

截图里若已直接看到「仍要运行」，点它即可。

### 360 安全卫士

1. 弹窗选 **「允许程序所有操作」** / **「信任」**
2. 或：360 → 病毒查杀 → 信任区 → 添加 `Lighting-*.exe` 所在目录  
3. 可选：关闭对该目录的「下载防护 / 云查杀」误报干扰（自行权衡）

请只从本仓库 [GitHub Releases](https://github.com/a3165458/lighting/releases) 下载，不要用来路不明的镜像站。

---

## 真正的解决方案（开发者 / 维护者）

### 1. Windows 代码签名证书（推荐）

购买 **OV 代码签名证书**（DigiCert / Sectigo / SSL.com 等，约每年几百元人民币量级）。  
自 2024 年起，EV 也不再「一签就立刻免 SmartScreen」，但 **签名后**：

- 会显示真实 **发布者名称**（不再是「未知」）
- 信誉可随同一证书持续积累，后续版本 SoftScreen 会少很多
- 360 等杀软误报也会明显下降

把证书配进 GitHub Actions Secrets 后，Release 流水线会自动签名：

| Secret | 含义 |
|--------|------|
| `WINDOWS_CSC_LINK` | `.pfx` 文件的 **Base64** |
| `WINDOWS_CSC_KEY_PASSWORD` | PFX 密码 |

生成 Base64（在你保管证书的电脑上）：

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes(".\lighting-codesign.pfx")) | Set-Clipboard
```

推送新 tag 发版后，用资源管理器右键 → 属性 → 数字签名，应能看到发布者。

也可使用 [Azure Trusted Signing](https://learn.microsoft.com/windows/apps/package-and-deploy/code-signing-options)（按量计费、云端签名，适合没有 USB Key 的 CI）。

### 2. 向微软 / 360 提交误报

签名之后仍被拦时：

- Microsoft：https://www.microsoft.com/wdsi/filesubmission  
- 360：https://open.soft.360.cn/（或杀软内「误报反馈」）

附上 GitHub Release 链接与文件 SHA256。

### 3. 无法立刻买证书时

- 继续用「仍要运行」+ 360 加信任（见上文）
- 优先用 **安装包** 而非便携版（安装路径固定，便于加白名单）
- 不要对 exe 再套一层未知壳/压缩加壳（更容易被报毒）

---

## 与华硕 GlideX 的差别（为何它「开箱即用」）

| | 华硕 GlideX | Lighting |
|--|-------------|----------|
| 虚拟屏驱动 | **自家签名的 IddCx 驱动**，随安装包安装 | 依赖开源 **MttVDD**（社区驱动） |
| 取帧方式 | 驱动 SwapChain 直接出帧再编码 | 先造虚拟显示器，再 DXGI 抓屏编码 |
| 代码签名 | 华硕正版签名，SmartScreen/杀软放行 | 开源未签名 → 易被 360/SmartScreen 拦 |
| 扩展失败时 | 驱动一体化，极少失败 | 驱动/权限/杀软任一环节失败就会报错 |

因此：**不是「GlideX 没驱动」**，而是驱动被做成了产品的一部分且有商业签名。  
Lighting 要达到同级体验，长期需要自研/采购可签名的 IddCx 驱动，并配置 Authenticode 证书——这超出「改几行业务代码」的范围。

**当前务实策略（v0.1.11）：**

1. 默认 **镜像主屏（免驱动）**，保证第一次就能投屏  
2. 「扩展 / 仅投扩展」仍可用，失败时 **自动改镜像并继续**，不再卡在红字错误  
3. 代码签名：需维护者自备 OV 证书（见上文 Secrets），AI 无法代签受信任证书  
