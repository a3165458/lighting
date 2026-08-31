# Build Android debug APK and stage as Lighting.apk for Electron bundle.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$AndroidDir = Join-Path $Root "android"
$ResDir = Join-Path $Root "host-ui\resources"
$ApkOut = Join-Path $ResDir "Lighting.apk"
$GradleBat = Join-Path $AndroidDir "gradlew.bat"

if (-not (Test-Path $GradleBat)) {
    throw "Missing android\gradlew.bat"
}

# Resolve Android SDK for Gradle local.properties
$sdk = $env:ANDROID_HOME
if (-not $sdk) { $sdk = $env:ANDROID_SDK_ROOT }
if (-not $sdk) {
    $default = Join-Path $env:LOCALAPPDATA "Android\Sdk"
    if (Test-Path $default) { $sdk = $default }
}
if (-not $sdk) {
    throw "ANDROID_HOME not set. Install Android SDK or set ANDROID_HOME."
}

$localProps = Join-Path $AndroidDir "local.properties"
$sdkPath = $sdk -replace '\\', '/'
[System.IO.File]::WriteAllText($localProps, "sdk.dir=$sdkPath`n")

Write-Host "==> Gradle assembleDebug"
Push-Location $AndroidDir
try {
    & $GradleBat assembleDebug --no-daemon
    if ($LASTEXITCODE -ne 0) { throw "gradle assembleDebug failed" }
} finally {
    Pop-Location
}

$built = Join-Path $AndroidDir "app\build\outputs\apk\debug\app-debug.apk"
if (-not (Test-Path $built)) {
    throw "APK not found: $built"
}

New-Item -ItemType Directory -Force -Path $ResDir | Out-Null
Copy-Item -Force $built $ApkOut
Write-Host "Staged APK -> $ApkOut ($(('{0:N1}' -f ((Get-Item $ApkOut).Length / 1MB)) MB)"
