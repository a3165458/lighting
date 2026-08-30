# One-click package for beginners: portable exe with embedded host.
# Output: host-ui\release\Lighting副屏-*-便携版.exe
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

Write-Host "==> Build lighting-host.exe"
& (Join-Path $Root "scripts\build-windows.ps1")
if ($LASTEXITCODE -ne 0) { throw "host build failed" }

$hostExe = Join-Path $Root "host-windows\target\release\lighting-host.exe"
if (-not (Test-Path $hostExe)) { throw "missing $hostExe" }

$resDir = Join-Path $Root "host-ui\resources"
New-Item -ItemType Directory -Force -Path $resDir | Out-Null
Copy-Item -Force $hostExe (Join-Path $resDir "lighting-host.exe")

Set-Location (Join-Path $Root "host-ui")
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm not found. Install Node.js LTS first."
}

Write-Host "==> npm install"
npm install
if ($LASTEXITCODE -ne 0) { throw "npm install failed" }

Write-Host "==> electron portable + setup"
npm run electron:build:win
if ($LASTEXITCODE -ne 0) { throw "electron build failed" }

Write-Host ""
Write-Host "Done. Give beginners this file:"
Get-ChildItem .\release -Filter "*便携版.exe" | ForEach-Object {
    Write-Host ("  " + $_.FullName)
}
Get-ChildItem .\release -Filter "*安装包.exe" | ForEach-Object {
    Write-Host ("  " + $_.FullName)
}
Write-Host ""
Write-Host "Double-click the portable exe. First launch auto-downloads adb + ffmpeg."
