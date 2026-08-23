$ErrorActionPreference = "SilentlyContinue"
Get-Process -Name lighting-host -ErrorAction SilentlyContinue | Stop-Process -Force
Get-CimInstance Win32_Process -Filter "Name='ffmpeg.exe'" | Where-Object { $_.CommandLine -match "ddagrab|gdigrab" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
Write-Host "已尝试停止 Lighting 主机。"
