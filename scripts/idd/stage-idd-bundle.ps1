# Stage LightingIdd (IddCx) artifacts into host-ui/resources/idd for portable bundling.
# Expects a pre-built LightingIdd.dll (WDK build on Windows). Downloads nefconw helper.
$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$outDir = Join-Path $Root 'host-ui\resources\idd'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

Copy-Item (Join-Path $PSScriptRoot 'provision.ps1') (Join-Path $outDir 'provision.ps1') -Force
Copy-Item (Join-Path $Root 'driver-idd\LightingIdd.inf') (Join-Path $outDir 'LightingIdd.inf') -Force

# Prefer Release x64 build outputs from common WDK / VS layouts.
$dllCandidates = @(
    (Join-Path $Root 'driver-idd\x64\Release\LightingIdd.dll'),
    (Join-Path $Root 'driver-idd\Release\LightingIdd.dll'),
    (Join-Path $Root 'driver-idd\src\x64\Release\LightingIdd.dll'),
    (Join-Path $env:LIGHTING_IDD_DLL 'LightingIdd.dll'),
    $env:LIGHTING_IDD_DLL
) | Where-Object { $_ -and (Test-Path $_) }

if (-not $dllCandidates) {
    Write-Warning @"
LightingIdd.dll not found — IddCx bundle will be INF-only (install will fail until you build the driver).
Build driver-idd\LightingIdd.sln (Release|x64) with VS2022+WDK, then re-run this script.
Or set LIGHTING_IDD_DLL to the full path of LightingIdd.dll.
"@
} else {
    Copy-Item $dllCandidates[0] (Join-Path $outDir 'LightingIdd.dll') -Force
    Write-Host "Staged DLL from $($dllCandidates[0])"
}

$NefConURL = 'https://github.com/nefarius/nefcon/releases/download/v1.14.0/nefcon_v1.14.0.zip'
$temp = Join-Path $env:TEMP ('LightingIddStage-' + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $temp | Out-Null
try {
    $nefZip = Join-Path $temp 'nefcon.zip'
    Invoke-WebRequest -Uri $NefConURL -OutFile $nefZip -UseBasicParsing
    Expand-Archive -Path $nefZip -DestinationPath $temp -Force
    $nef = Get-ChildItem -Path $temp -Recurse -Filter 'nefconw.exe' | Select-Object -First 1
    if (-not $nef) { throw 'nefconw.exe not found' }
    Copy-Item $nef.FullName (Join-Path $outDir 'nefconw.exe') -Force
} finally {
    Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "IddCx bundle staged at $outDir"
Get-ChildItem $outDir | Format-Table Name, Length
