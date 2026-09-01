# Lighting IddCx driver provisioning (ASCII result only).
# Writes "<STATUS>|<DETAIL>" to -ResultFile. STATUS is OK or FAIL.
#
# Option B: enable Root\LightingIdd (our IddCx UMDF). Monitor appears on D0.
param(
    [Parameter(Mandatory = $true)]
    [string]$BundleDir,
    [Parameter(Mandatory = $true)]
    [string]$ResultFile,
    [ValidateSet('Full', 'EnableOnly')]
    [string]$Mode = 'Full'
)

$ErrorActionPreference = 'Stop'
$HwId = 'Root\LightingIdd'

function Write-Result([string]$Status, [string]$Detail) {
    $safe = ($Detail -replace '[^a-zA-Z0-9_\-:.]', '_')
    if ($safe.Length -gt 120) { $safe = $safe.Substring(0, 120) }
    $parent = Split-Path -Parent $ResultFile
    if ($parent -and -not (Test-Path $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    Set-Content -Path $ResultFile -Value ($Status + '|' + $safe) -Encoding ASCII -NoNewline
}

function Find-LightingIddDevice {
    try {
        $device = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
            ($_.InstanceId -like '*LightingIdd*') -or
            ($_.HardwareID -like '*LightingIdd*') -or
            (($_.FriendlyName) -and ($_.FriendlyName -match 'Lighting Virtual Display'))
        } | Select-Object -First 1
        if (-not $device) {
            $device = Get-PnpDevice -HardwareID $HwId -ErrorAction SilentlyContinue | Select-Object -First 1
        }
        return $device
    } catch {
        return $null
    }
}

function Enable-LightingIddDevice {
    $device = Find-LightingIddDevice
    if (-not $device) {
        try { pnputil /enable-device /deviceid $HwId 2>&1 | Out-Null } catch {}
        $device = Find-LightingIddDevice
    }
    if (-not $device) { return $null }
    try { Enable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false -ErrorAction SilentlyContinue } catch {}
    try { pnputil /enable-device $device.InstanceId 2>&1 | Out-Null } catch {}
    try { pnputil /restart-device $device.InstanceId 2>&1 | Out-Null } catch {}
    Start-Sleep -Seconds 3
    return (Find-LightingIddDevice)
}

function Test-PnpSuccess([int]$ExitCode) {
    return ($ExitCode -eq 0 -or $ExitCode -eq 259)
}

function Install-FromBundle([string]$Dir) {
    if (-not (Test-Path $Dir)) { throw 'BUNDLE_DIR_MISSING' }

    $inf = Get-ChildItem -Path $Dir -Recurse -Filter 'LightingIdd.inf' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $inf) { throw 'BUNDLE_INF_MISSING' }

    $dll = Get-ChildItem -Path $Dir -Recurse -Filter 'LightingIdd.dll' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $dll) { throw 'BUNDLE_DLL_MISSING' }

    $infDir = $inf.DirectoryName
    $nef = Get-ChildItem -Path $Dir -Recurse -Filter 'nefconw.exe' -ErrorAction SilentlyContinue | Select-Object -First 1

    $added = $false
    $pnputilEc = -1
    try {
        pnputil /add-driver $inf.FullName /install 2>&1 | Out-Null
        $pnputilEc = $LASTEXITCODE
        if (Test-PnpSuccess $pnputilEc) { $added = $true }
    } catch {
        $pnputilEc = $LASTEXITCODE
    }

    # Create root device node if needed (same pattern as sample / GlideX-class installers).
    if ($nef) {
        Push-Location $infDir
        try {
            & $nef.FullName install $inf.Name $HwId 2>&1 | Out-Null
            if (Test-PnpSuccess $LASTEXITCODE) { $added = $true }
        } catch {}
        finally {
            Pop-Location
        }
    }

    if (-not $added -and -not (Find-LightingIddDevice)) {
        throw "DRIVER_INSTALL_FAILED:pnputil=$pnputilEc"
    }
    Start-Sleep -Seconds 4
}

function Map-ProvisionError([string]$Message) {
    if ($Message -match 'BUNDLE_INF_MISSING|BUNDLE_DLL_MISSING|BUNDLE_DIR_MISSING|DRIVER_INSTALL_FAILED|DEVICE') {
        return $Matches[0]
    }
    if ($Message -match 'access|denied|elevation|administrator|0x5|5\)') {
        return 'ACCESS_DENIED'
    }
    if ($Message -match 'sign|certificate|catalog|trust|blocked|Code Integrity') {
        return 'DRIVER_SIGNATURE'
    }
    $slug = ($Message -replace '[^a-zA-Z0-9_ ]', '') -replace '\s+', '_'
    if ($slug.Length -gt 48) { $slug = $slug.Substring(0, 48) }
    if ([string]::IsNullOrWhiteSpace($slug)) { return 'UNEXPECTED' }
    return "ERR_$slug"
}

try {
    if ($Mode -eq 'EnableOnly') {
        $dev = Enable-LightingIddDevice
        if ($dev) {
            Write-Result 'OK' ('ENABLED:' + $(if ($dev.InstanceId) { $dev.InstanceId } else { 'unknown' }))
            exit 0
        }
        Write-Result 'FAIL' 'DEVICE_NOT_FOUND'
        exit 1
    }

    $dev = Enable-LightingIddDevice
    if (-not $dev) {
        Install-FromBundle -Dir $BundleDir
        $dev = Enable-LightingIddDevice
    }

    if (-not $dev) {
        Write-Result 'FAIL' 'DEVICE_STILL_MISSING'
        exit 1
    }

    Write-Result 'OK' ('READY:' + $(if ($dev.InstanceId) { $dev.InstanceId } else { 'unknown' }))
    exit 0
} catch {
    Write-Result 'FAIL' (Map-ProvisionError $_.Exception.Message)
    exit 1
}
