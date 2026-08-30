# 在 Windows 本机编译 lighting-host.exe
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $Root "host-windows")

$vcvars = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    $vcvars = "${env:ProgramFiles}\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
}
if (-not (Test-Path $vcvars)) {
    throw "未找到 VS 2022 C++ 工具链（vcvars64.bat）。请安装「使用 C++ 的桌面开发」工作负载。"
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "未找到 cargo。请先安装 Rust：https://rustup.rs/"
}

Write-Host "正在 release 编译 lighting-host.exe …"
cmd /c "`"$vcvars`" && cargo build --release"
if ($LASTEXITCODE -ne 0) { throw "cargo build --release 失败" }

$exe = Join-Path $Root "host-windows\target\release\lighting-host.exe"
Write-Host ""
Write-Host "编译完成："
Write-Host "  $exe"
Write-Host ""
Write-Host "运行： & `"$exe`""
Write-Host "或：  .\scripts\start.ps1"
