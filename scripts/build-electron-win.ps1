# Build Electron Windows exe for Lighting UI shell.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location (Join-Path $Root "host-ui")

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm not found. Install Node.js LTS first."
}

Write-Host "Installing dependencies..."
npm install
if ($LASTEXITCODE -ne 0) { throw "npm install failed" }

Write-Host "Building Windows Electron package..."
npm run electron:build:win
if ($LASTEXITCODE -ne 0) { throw "electron build failed" }

Write-Host ""
Write-Host "Build OK. Output folder:"
Write-Host "  $(Join-Path (Get-Location) 'release')"
Get-ChildItem .\release -Filter *.exe | ForEach-Object { Write-Host "  $($_.FullName)" }
