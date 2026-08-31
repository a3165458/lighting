$ErrorActionPreference = "Stop"
Write-Host "自动准备 Virtual Display Driver（扩展屏用，通常由 Lighting 在首次扩展时调用）…"
winget install --id=VirtualDrivers.Virtual-Display-Driver -e --accept-package-agreements --accept-source-agreements --disable-interactivity
Write-Host "完成。可在 Lighting 中选择「扩展屏」或「仅第二屏」，也可用 Win+P。"
