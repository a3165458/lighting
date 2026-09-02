# Stage LightingIdd (IddCx) artifacts into host-ui/resources/idd for portable bundling.
# Optional for product path A (mirror @ tablet resolution). Missing DLL must not fail CI.
$ErrorActionPreference = 'Stop'

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$outDir = Join-Path $Root 'host-ui\resources\idd'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

Copy-Item (Join-Path $PSScriptRoot 'provision.ps1') (Join-Path $outDir 'provision.ps1') -Force
Copy-Item (Join-Path $Root 'driver-idd\LightingIdd.inf') (Join-Path $outDir 'LightingIdd.inf') -Force

# Prefer Release x64 build outputs from common WDK / VS layouts.
# Do not Join-Path a null env var — PowerShell throws before Where-Object can filter.
$dllCandidates = @(
    (Join-Path $Root 'driver-idd\x64\Release\LightingIdd.dll'),
    (Join-Path $Root 'driver-idd\Release\LightingIdd.dll'),
    (Join-Path $Root 'driver-idd\src\x64\Release\LightingIdd.dll')
)
if (-not [string]::IsNullOrWhiteSpace($env:LIGHTING_IDD_DLL)) {
    $dllCandidates += @(
        (Join-Path $env:LIGHTING_IDD_DLL 'LightingIdd.dll'),
        $env:LIGHTING_IDD_DLL
    )
}
$dllCandidates = @($dllCandidates | Where-Object { $_ -and (Test-Path $_) })

if ($dllCandidates.Count -eq 0) {
    Write-Warning 'LightingIdd.dll not found — staging INF + provision only (path A does not need this driver).'
} else {
    Copy-Item $dllCandidates[0] (Join-Path $outDir 'LightingIdd.dll') -Force
    Write-Host "Staged DLL from $($dllCandidates[0])"
}

# nefconw is only needed when installing the driver; skip quietly if download fails
# so path-A releases still succeed without WDK artifacts.
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
    if ($nef) {
        Copy-Item $nef.FullName (Join-Path $outDir $nef.Name) -Force
    } else {
        Write-Warning 'nefconc/nefconw not found in nefcon zip — IDD install helper omitted'
    }
    $win = Get-ChildItem -Path $temp -Recurse -Filter 'nefconw.exe' | Select-Object -First 1
    if ($win -and (-not $nef -or $win.Name -ne $nef.Name)) {
        Copy-Item $win.FullName (Join-Path $outDir $win.Name) -Force
    }
} catch {
    Write-Warning "nefcon download skipped: $($_.Exception.Message)"
} finally {
    Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "IddCx bundle staged at $outDir"
Get-ChildItem $outDir | Format-Table Name, Length
exit 0
