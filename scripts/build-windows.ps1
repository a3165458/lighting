# Build lighting-host.exe on Windows (MSVC required).
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $Root "host-windows")

$vcvars = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (-not (Test-Path $vcvars)) {
    $vcvars = "${env:ProgramFiles}\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
}
if (-not (Test-Path $vcvars)) {
    $vcvars = "${env:ProgramFiles}\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat"
}
if (-not (Test-Path $vcvars)) {
    throw "vcvars64.bat not found. Install VS 2022 C++ desktop workload first."
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not found. Install Rust from https://rustup.rs/"
}

Write-Host "Building lighting-host.exe (release)..."
cmd /c "`"$vcvars`" && cargo build --release"
if ($LASTEXITCODE -ne 0) {
    throw "cargo build --release failed"
}

$exe = Join-Path $Root "host-windows\target\release\lighting-host.exe"
Write-Host ""
Write-Host "Build OK:"
Write-Host "  $exe"
Write-Host ""
Write-Host "Run:"
Write-Host "  $exe"
Write-Host "or:"
Write-Host "  .\scripts\start.ps1"
