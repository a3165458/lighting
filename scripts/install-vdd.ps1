$ErrorActionPreference = "Stop"
Write-Host "安装 Virtual Display Driver（Windows 虚拟显示器，用于扩展屏）…"
winget install --id=VirtualDrivers.Virtual-Display-Driver -e --accept-package-agreements --accept-source-agreements
Write-Host "安装完成后请打开 Windows 显示设置：设为「扩展这些显示器」，建议分辨率 2560×1440。"
