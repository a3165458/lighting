# Windows SmartScreen / 360 报毒

当前 Release **没有 Authenticode 签名**，所以会出现：

- Windows：「已保护你的电脑 / 发布者未知」
- 360：「发现病毒 / 木马 / 危险」

这是未签名软件 + 会安装虚拟显示驱动时的典型误报，**不是程序里带了木马**。请只从本仓库 [GitHub Releases](https://github.com/a3165458/lighting/releases/latest) 下载。

---

## 你现在怎么用（临时）

### 360 安全卫士

1. 弹窗选 **允许程序所有操作** / **信任**
2. 360 → **病毒查杀** → **信任区** → 把安装目录加进去  
   常见路径：`C:\Program Files\Lighting副屏\` 或便携版解压目录
3. 可选：对该目录关掉「下载防护 / 云查杀」（自行权衡）

### Microsoft Defender SmartScreen

1. 点 **更多信息**
2. 再点 **仍要运行**

优先用 **安装包**（路径固定，方便加白名单），不要再套壳压缩。

---

## 彻底解决（维护者）

360 要同时做两件事：**代码签名** + **送 360 加白**。只改代码、只重新打包，杀软信誉不会变。

### 1. 买 Windows 代码签名证书并签每一个 exe

需要 **OV 代码签名证书**（公司主体；个人 IV 证书对 360/SmartScreen 帮助很小）。

国内常见渠道：沃通、天威诚信、锐成，或 Sectigo / SSL.com / DigiCert 代理。  
2023 年起新证私钥多半在 UKey / HSM 上，**不能再导出成普通 PFX 塞进 GitHub**。

开源项目也可申请 [SignPath Foundation](https://signpath.org/) **免费**代签（证书显示为 SignPath Foundation）。申请条件与本仓库政策见 [`docs/CODE-SIGNING.md`](docs/CODE-SIGNING.md)。

适合本仓库自动发版的方式：

| 方案 | 说明 |
|------|------|
| [Azure Trusted Signing](https://learn.microsoft.com/windows/apps/package-and-deploy/trusted-signing) | 云端签名，GitHub Actions 可直接用，按量计费 |
| SSL.com eSigner / SignPath | 云端签名，适合没有 USB Key 的 CI |
| 传统 OV 证书 + USB Key | 只能在插着 Key 的 Windows 上本地签，不适合现在的云打包 |

发版流水线已经预留了 electron-builder 签名环境变量。若你仍持有可导出的 `.pfx`：

| GitHub Secret | 含义 |
|----------------|------|
| `WINDOWS_CSC_LINK` | `.pfx` 的 Base64 |
| `WINDOWS_CSC_KEY_PASSWORD` | PFX 密码 |

本机生成 Base64：

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes(".\lighting-codesign.pfx")) | Set-Clipboard
```

签完后应满足：

- 安装包、便携版、里面的 `lighting-host.exe` **都有数字签名**
- 资源管理器 → 右键 exe → 属性 → **数字签名** 能看到发布者名称

配好 Secret 或云签名后重新跑 Release，不要继续发未签名包。

### 2. 向 360 送检加白（国内必须做）

签名之后 360 仍可能拦几天到几周，需要送检：

1. 打开 [360 软件开放平台](https://open.soft.360.cn/) 提交软件认证  
2. 或在 360 客户端：病毒查杀 → 查看报告 → **误报反馈**
3. 附上：
   - https://github.com/a3165458/lighting/releases/latest
   - 文件 SHA256（Release 里的 `SHA256SUMS.txt`）
   - 说明：开源投屏工具，会安装微软已签名的虚拟显示驱动（MttVDD）

微软误报提交：https://www.microsoft.com/wdsi/filesubmission

### 3. 还没买证书时

- 继续用「允许 / 信任区」
- 不要把 exe 再加壳、不要从网盘镜像下
- 装好后把安装目录加进 360 信任区，一般只拦第一次

---

## 和华硕 GlideX 的差别

| | 华硕 GlideX | Lighting |
|--|-------------|----------|
| 程序签名 | 华硕正版签名 | 目前未签名 |
| 虚拟屏驱动 | 自家签名的 IddCx | 开源 MttVDD（驱动本身已有目录签名） |
| 360 / SmartScreen | 开箱可用 | 未签名 + 装驱动 → 易被启发式拦截 |

360 认的是 **发布者证书信誉**，不是「功能写对了就不报」。GlideX 能开箱即用，是因为华硕签过名并已在杀软库里。
