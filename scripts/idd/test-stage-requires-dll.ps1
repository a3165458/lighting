# CI / local assertion: stage-idd-bundle.ps1 must fail when LightingIdd.dll is absent.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$stage = Join-Path $here 'stage-idd-bundle.ps1'
$assert = Join-Path $here 'assert-idd-bundle.ps1'
if (-not (Test-Path $stage)) { throw "missing $stage" }

$tempRoot = if (-not [string]::IsNullOrWhiteSpace($env:TEMP)) { $env:TEMP } elseif (-not [string]::IsNullOrWhiteSpace($env:TMPDIR)) { $env:TMPDIR } else { '/tmp' }
$temp = Join-Path $tempRoot ('LightingIddStageTest-' + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $temp | Out-Null
try {
    $repo = Join-Path $temp 'repo'
    $out = Join-Path (Join-Path (Join-Path $repo 'host-ui') 'resources') 'idd'
    $driverIdd = Join-Path $repo 'driver-idd'
    New-Item -ItemType Directory -Force -Path $driverIdd | Out-Null
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    $srcInf = Join-Path (Join-Path (Split-Path -Parent (Split-Path -Parent $here)) 'driver-idd') 'LightingIdd.inf'
    Copy-Item $srcInf (Join-Path $driverIdd 'LightingIdd.inf') -Force
    # Plant an INF-only leftover the way the old pack did.
    Copy-Item (Join-Path $driverIdd 'LightingIdd.inf') (Join-Path $out 'LightingIdd.inf') -Force

    $failed = $false
    try {
        & $stage -RepoRoot $repo -OutDir $out
        if ($LASTEXITCODE -eq 0) {
            throw 'stage-idd-bundle.ps1 exited 0 without LightingIdd.dll'
        }
        $failed = $true
    } catch {
        $failed = $true
        Write-Host "stage failed as required: $($_.Exception.Message)"
    }
    if (-not $failed) {
        throw 'expected stage-idd-bundle.ps1 to fail when DLL is missing'
    }

    # AllowMissingDll must strip the leftover INF so extraResources cannot ship a fake installer.
    & $stage -RepoRoot $repo -OutDir $out -AllowMissingDll
    if ($LASTEXITCODE -ne 0) { throw "AllowMissingDll should exit 0, got $LASTEXITCODE" }
    if (Test-Path (Join-Path $out 'LightingIdd.inf')) {
        throw 'AllowMissingDll left LightingIdd.inf in the output dir'
    }
    & $assert -BundleDir $out -ForbidIncomplete
    Write-Host 'OK: stage-idd-bundle.ps1 fails without LightingIdd.dll and does not leave INF-only output'
} finally {
    Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
}
