# One-click package: host + Android APK + Electron portable/setup.
# Output: host-ui\release\Lighting副屏-*-便携版.exe
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot

Write-Host "==> Build Android APK (Lighting.apk)"
& (Join-Path $Root "scripts\build-android.ps1")
if ($LASTEXITCODE -ne 0) { throw "android build failed" }

Write-Host "==> Build lighting-host.exe"
& (Join-Path $Root "scripts\build-windows.ps1")
if ($LASTEXITCODE -ne 0) { throw "host build failed" }

$hostExe = Join-Path $Root "host-windows\target\release\lighting-host.exe"
if (-not (Test-Path $hostExe)) { throw "missing $hostExe" }

$resDir = Join-Path $Root "host-ui\resources"
New-Item -ItemType Directory -Force -Path $resDir | Out-Null
Copy-Item -Force $hostExe (Join-Path $resDir "lighting-host.exe")

if (-not (Test-Path (Join-Path $resDir "Lighting.apk"))) {
    throw "missing host-ui\resources\Lighting.apk"
}

Write-Host "==> Stage virtual display driver bundle (MttVDD + nefconw)"
& (Join-Path $Root "scripts\vdd\stage-vdd-bundle.ps1")
if ($LASTEXITCODE -ne 0) { throw "vdd bundle staging failed" }

Write-Host "==> Build LightingIdd (WDK) when possible, then stage a complete Idd bundle"
$iddBuild = Join-Path $Root "scripts\idd\build-idd.ps1"
$iddStage = Join-Path $Root "scripts\idd\stage-idd-bundle.ps1"
$iddAssert = Join-Path $Root "scripts\idd\assert-idd-bundle.ps1"
$iddBuilt = $false
try {
    & $iddBuild
    if ($LASTEXITCODE -eq 0) { $iddBuilt = $true }
} catch {
    Write-Warning "LightingIdd WDK build skipped: $($_.Exception.Message)"
}
if ($iddBuilt) {
    & $iddStage
    if ($LASTEXITCODE -ne 0) { throw "idd bundle staging failed" }
    & $iddAssert -RequireComplete
    if ($LASTEXITCODE -ne 0) { throw "idd bundle assertion failed" }
} else {
    Write-Warning "No LightingIdd.dll — omitting Idd from this pack (runtime uses MttVDD). INF-only Idd is forbidden."
    & $iddStage -AllowMissingDll
    if ($LASTEXITCODE -ne 0) { throw "idd omit staging failed" }
    & $iddAssert -ForbidIncomplete
    if ($LASTEXITCODE -ne 0) { throw "idd bundle assertion failed" }
}

Set-Location (Join-Path $Root "host-ui")
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    throw "npm not found. Install Node.js LTS first."
}

Write-Host "==> npm install"
npm install
if ($LASTEXITCODE -ne 0) { throw "npm install failed" }

Write-Host "==> electron portable + setup (bundled host + APK)"
npm run electron:build:win
if ($LASTEXITCODE -ne 0) { throw "electron build failed" }

Write-Host ""
Write-Host "Done. Beginner bundle includes lighting-host.exe + Lighting.apk:"
Get-ChildItem .\release -Filter "*便携版.exe" | ForEach-Object {
    Write-Host ("  " + $_.FullName)
}
Get-ChildItem .\release -Filter "*安装包.exe" | ForEach-Object {
    Write-Host ("  " + $_.FullName)
}
