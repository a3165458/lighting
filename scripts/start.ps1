$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Find-Adb {
    $cmd = Get-Command adb -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $local = Join-Path $Root ".runtime\platform-tools\adb.exe"
    if (Test-Path $local) { return $local }
    $sdk = Join-Path $env:LOCALAPPDATA "Android\Sdk\platform-tools\adb.exe"
    if (Test-Path $sdk) { return $sdk }
    return $null
}

function Invoke-VsDev {
    param([string]$Command)
    $vcvars = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
    if (-not (Test-Path $vcvars)) {
        throw "未找到 VS 2022 Build Tools（vcvars64.bat）。请安装「使用 C++ 的桌面开发」工作负载。"
    }
    cmd /c "`"$vcvars`" && $Command"
    if ($LASTEXITCODE -ne 0) { throw "命令失败: $Command" }
}

if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
    throw "未找到 ffmpeg。请安装并加入 PATH。"
}

$adb = Find-Adb
if (-not $adb) {
    Write-Host "未找到 adb，正在下载 Google Platform-Tools 到 .runtime …"
    New-Item -ItemType Directory -Force -Path (Join-Path $Root ".runtime") | Out-Null
    $zip = Join-Path $Root ".runtime\platform-tools.zip"
    Invoke-WebRequest -Uri "https://dl.google.com/android/repository/platform-tools-latest-windows.zip" -OutFile $zip
    Expand-Archive -Path $zip -DestinationPath (Join-Path $Root ".runtime") -Force
    $adb = Join-Path $Root ".runtime\platform-tools\adb.exe"
}
Write-Host "adb: $adb"
& $adb devices

$exe = Join-Path $Root "host-windows\target\release\lighting-host.exe"
if (-not (Test-Path $exe)) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "未找到 cargo。请先安装 Rust：https://rustup.rs/"
    }
    Write-Host "正在编译 Lighting 主机…"
    Set-Location (Join-Path $Root "host-windows")
    Invoke-VsDev "cargo build --release"
    Set-Location $Root
}

Write-Host "启动 Lighting 主机…"
& $exe
