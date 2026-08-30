# Build Electron Windows exe and bundle lighting-host.exe for IPC.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

Write-Host "1) Building lighting-host.exe ..."
& (Join-Path $Root "scripts\build-windows.ps1")
if ($LASTEXITCODE -ne 0) { throw "host build failed" }

$hostExe = Join-Path $Root "host-windows\target\release\lighting-host.exe"
if (-not (Test-Path $hostExe)) {
    throw "missing $hostExe"
}

$resDir = Join-Path $Root "host-ui\resources"
New-Item -ItemType Directory -Force -Path $resDir | Out-Null
Copy-Item -Force $hostExe (Join-Path $resDir "lighting-host.exe")
Write-Host "Copied lighting-host.exe into host-ui\resources\"

Set-Location (Join-Path $Root "host-ui")
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm not found. Install Node.js LTS first."
}

Write-Host "2) npm install ..."
npm install
if ($LASTEXITCODE -ne 0) { throw "npm install failed" }

Write-Host "3) electron-builder (Windows) ..."
npm run electron:build:win
if ($LASTEXITCODE -ne 0) { throw "electron build failed" }

Write-Host ""
Write-Host "Build OK. Output:"
Get-ChildItem .\release -Filter *.exe | ForEach-Object { Write-Host "  $($_.FullName)" }
