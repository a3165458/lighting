# Stage LightingIdd (IddCx) artifacts into host-ui/resources/idd for portable bundling.
# A complete bundle is INF + LightingIdd.dll + nefconc.exe (and .cat when WDK produced it).
# INF-only staging is forbidden: missing DLL must fail this script (and therefore the pack job).
param(
    [string]$RepoRoot = '',
    [string]$OutDir = '',
    [switch]$AllowMissingDll
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
}
if ([string]::IsNullOrWhiteSpace($OutDir)) {
    $OutDir = Join-Path $RepoRoot 'host-ui\resources\idd'
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$provisionSrc = Join-Path $PSScriptRoot 'provision.ps1'
if (-not (Test-Path $provisionSrc)) {
    throw "missing $provisionSrc"
}
Copy-Item $provisionSrc (Join-Path $OutDir 'provision.ps1') -Force

$infSrc = Join-Path $RepoRoot 'driver-idd\LightingIdd.inf'
if (-not (Test-Path $infSrc)) {
    throw "missing $infSrc"
}

# Prefer Release x64 build outputs from common WDK / VS layouts.
# Do not Join-Path a null env var — PowerShell throws before Where-Object can filter.
$dllCandidates = @(
    (Join-Path $RepoRoot 'driver-idd\x64\Release\LightingIdd.dll'),
    (Join-Path $RepoRoot 'driver-idd\LightingIdd\x64\Release\LightingIdd.dll'),
    (Join-Path $RepoRoot 'driver-idd\Release\LightingIdd.dll'),
    (Join-Path $RepoRoot 'driver-idd\src\x64\Release\LightingIdd.dll')
)
if (-not [string]::IsNullOrWhiteSpace($env:LIGHTING_IDD_DLL)) {
    $dllCandidates += @(
        (Join-Path $env:LIGHTING_IDD_DLL 'LightingIdd.dll'),
        $env:LIGHTING_IDD_DLL
    )
}
$dllCandidates = @($dllCandidates | Where-Object { $_ -and (Test-Path $_) })

function Remove-IncompleteIddInf([string]$Dir) {
    $leftover = Join-Path $Dir 'LightingIdd.inf'
    if (Test-Path $leftover) {
        Remove-Item $leftover -Force
        Write-Warning "Removed INF-only $leftover so Electron extraResources cannot ship a fake Idd installer."
    }
}

if ($dllCandidates.Count -eq 0) {
    $msg = 'LightingIdd.dll not found — refusing INF-only Idd bundle. Build driver-idd (WDK/MSVC) first, or set LIGHTING_IDD_DLL.'
    Remove-IncompleteIddInf $OutDir
    if ($AllowMissingDll) {
        Write-Warning $msg
        Write-Warning 'AllowMissingDll: Idd omitted; portable must use the complete MttVDD bundle.'
        Write-Host "IddCx bundle omitted at $OutDir"
        Get-ChildItem $OutDir -ErrorAction SilentlyContinue | Format-Table Name, Length
        exit 0
    }
    throw $msg
}

Copy-Item $infSrc (Join-Path $OutDir 'LightingIdd.inf') -Force
Copy-Item $dllCandidates[0] (Join-Path $OutDir 'LightingIdd.dll') -Force
Write-Host "Staged DLL from $($dllCandidates[0])"

$dllDir = Split-Path -Parent $dllCandidates[0]
foreach ($extra in @('LightingIdd.cat', 'LightingIdd.sys', 'LightingIdd.pdb')) {
    $extraPath = Join-Path $dllDir $extra
    if (Test-Path $extraPath) {
        Copy-Item $extraPath (Join-Path $OutDir $extra) -Force
        Write-Host "Staged $extra"
    }
}

$NefConURL = 'https://github.com/nefarius/nefcon/releases/download/v1.14.0/nefcon_v1.14.0.zip'
$temp = Join-Path $env:TEMP ('LightingIddStage-' + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $temp | Out-Null
try {
    $nefZip = Join-Path $temp 'nefcon.zip'
    Invoke-WebRequest -Uri $NefConURL -OutFile $nefZip -UseBasicParsing
    Expand-Archive -Path $nefZip -DestinationPath $temp -Force
    $nef = Get-ChildItem -Path $temp -Recurse -Filter 'nefconc.exe' | Select-Object -First 1
    if (-not $nef) {
        $nef = Get-ChildItem -Path $temp -Recurse -Filter 'nefconw.exe' | Select-Object -First 1
    }
    if (-not $nef) {
        throw 'nefconc/nefconw not found in nefcon zip'
    }
    Copy-Item $nef.FullName (Join-Path $OutDir $nef.Name) -Force
    $win = Get-ChildItem -Path $temp -Recurse -Filter 'nefconw.exe' | Select-Object -First 1
    if ($win -and $win.Name -ne $nef.Name) {
        Copy-Item $win.FullName (Join-Path $OutDir $win.Name) -Force
    }
} finally {
    Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
}

$dllOut = Join-Path $OutDir 'LightingIdd.dll'
$nefOut = @(
    (Join-Path $OutDir 'nefconc.exe'),
    (Join-Path $OutDir 'nefconw.exe')
) | Where-Object { Test-Path $_ }
if (-not (Test-Path $dllOut)) {
    throw "staged bundle missing $dllOut"
}
if ($nefOut.Count -eq 0) {
    throw "staged bundle missing nefconc.exe (Idd Full install needs the device-node helper)"
}

Write-Host "IddCx bundle staged at $OutDir"
Get-ChildItem $OutDir | Format-Table Name, Length
exit 0
