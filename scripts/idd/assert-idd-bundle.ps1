# Assert the Electron Idd extraResources folder is not an INF-only fake installer.
param(
    [string]$BundleDir = '',
    [switch]$RequireComplete,
    [switch]$ForbidIncomplete
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($BundleDir)) {
    $root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    $BundleDir = Join-Path $root 'host-ui\resources\idd'
}

$inf = Join-Path $BundleDir 'LightingIdd.inf'
$dll = Join-Path $BundleDir 'LightingIdd.dll'
$nefconc = Join-Path $BundleDir 'nefconc.exe'
$nefconw = Join-Path $BundleDir 'nefconw.exe'
$hasInf = Test-Path $inf
$hasDll = Test-Path $dll
$hasNef = (Test-Path $nefconc) -or (Test-Path $nefconw)

if ($RequireComplete) {
    if (-not $hasDll) { throw "pack assertion failed: missing $dll" }
    if (-not $hasInf) { throw "pack assertion failed: missing $inf" }
    if (-not $hasNef) { throw "pack assertion failed: missing nefconc.exe in $BundleDir" }
    Write-Host "Idd bundle complete: LightingIdd.dll + LightingIdd.inf + nefconc"
    exit 0
}

if ($hasInf -and -not $hasDll) {
    throw "pack assertion failed: INF-only Idd bundle at $BundleDir (LightingIdd.dll missing). Refuse to ship."
}
if ($hasInf -and -not $hasNef) {
    throw "pack assertion failed: Idd INF present but nefconc.exe missing in $BundleDir"
}
if ($ForbidIncomplete -and $hasInf -and (-not $hasDll -or -not $hasNef)) {
    throw "pack assertion failed: incomplete Idd bundle in $BundleDir"
}

if ($hasDll) {
    Write-Host "Idd bundle has LightingIdd.dll"
} else {
    Write-Host "Idd bundle omitted (no LightingIdd.dll); portable must use MttVDD"
}
exit 0
