# Lighting IddCx driver provisioning (ASCII result only).
# Writes "<STATUS>|<DETAIL>" to -ResultFile. STATUS is OK or FAIL.
#
# Option B: enable Root\LightingIdd (our IddCx UMDF). Monitor appears on D0.
param(
    [string]$BundleDir = '',
    [string]$ResultFile = '',
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

function Test-IntendedVirtualHwid {
    param(
        [string]$InstanceId = '',
        [object]$HardwareIds = $null,
        [string]$FriendlyName = '',
        [Parameter(Mandatory = $true)]
        [string]$Token
    )
    $parts = @($InstanceId)
    if ($null -ne $HardwareIds) {
        $parts += @($HardwareIds | ForEach-Object { "$_" })
    }
    $idText = ($parts -join '|')
    $all = $idText + '|' + $FriendlyName
    if ($all -match '(?i)glidex') { return $false }
    $tok = [regex]::Escape($Token)
    return [bool]($idText -match "(?i)(^|[^A-Za-z0-9])$tok([^A-Za-z0-9]|$)")
}

function Test-ReadyVirtualDevice {
    param(
        $Device,
        [Parameter(Mandatory = $true)]
        [string]$Token
    )
    if (-not $Device) { return $false }
    if (-not (Test-IntendedVirtualHwid -InstanceId $Device.InstanceId -HardwareIds $Device.HardwareID -FriendlyName $Device.FriendlyName -Token $Token)) {
        return $false
    }
    $class = [string]$Device.Class
    $status = [string]$Device.Status
    if ($class -eq 'Unknown') { return $false }
    if ($status -match '(?i)Problem|Error|Disabled') { return $false }
    if ($null -ne $Device.ConfigManagerErrorCode -and [int]$Device.ConfigManagerErrorCode -ne 0) {
        return $false
    }
    return $true
}

function Should-RunNefconInstall([bool]$DevicePresent) {
    return -not $DevicePresent
}

function Find-LightingIddDevice {
    try {
        $found = @()
        try {
            $found += @(Get-PnpDevice -HardwareID $HwId -ErrorAction SilentlyContinue)
        } catch {}
        try {
            $found += @(Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
                Test-IntendedVirtualHwid -InstanceId $_.InstanceId -HardwareIds $_.HardwareID -FriendlyName $_.FriendlyName -Token 'LightingIdd'
            })
        } catch {}
        return ($found | Where-Object { $_ } | Select-Object -First 1)
    } catch {
        return $null
    }
}

function Enable-LightingIddDevice {
    $device = Find-LightingIddDevice
    if (-not $device) { return $null }
    try { Enable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false -ErrorAction SilentlyContinue } catch {}
    try { pnputil /enable-device $device.InstanceId 2>&1 | Out-Null } catch {}
    try { pnputil /restart-device $device.InstanceId 2>&1 | Out-Null } catch {}
    Start-Sleep -Seconds 3
    $device = Find-LightingIddDevice
    if (Test-ReadyVirtualDevice -Device $device -Token 'LightingIdd') { return $device }
    return $null
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
    $nef = Get-ChildItem -Path $Dir -Recurse -Filter 'nefconc.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $nef) {
        $nef = Get-ChildItem -Path $Dir -Recurse -Filter 'nefconw.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    if (-not $nef) {
        $nef = Get-ChildItem -Path $Dir -Recurse -Filter 'devcon.exe' -ErrorAction SilentlyContinue | Select-Object -First 1
    }

    $staged = $false
    $pnputilEc = -1
    try {
        pnputil /add-driver $inf.FullName /install 2>&1 | Out-Null
        $pnputilEc = $LASTEXITCODE
        if (Test-PnpSuccess $pnputilEc) { $staged = $true }
    } catch {
        $pnputilEc = $LASTEXITCODE
    }

    # Staging is not a device. Create Root\LightingIdd when the node is missing.
    if ((Should-RunNefconInstall ([bool](Find-LightingIddDevice))) -and $nef) {
        Push-Location $infDir
        try {
            & $nef.FullName install $inf.Name $HwId 2>&1 | Out-Null
        } catch {}
        finally {
            Pop-Location
        }
    }

    if (-not (Find-LightingIddDevice)) {
        throw "DRIVER_INSTALL_FAILED:pnputil=$pnputilEc,staged=$staged"
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

if ($MyInvocation.InvocationName -eq '.') {
    return
}
if ([string]::IsNullOrWhiteSpace($BundleDir) -or [string]::IsNullOrWhiteSpace($ResultFile)) {
    throw 'BundleDir and ResultFile are required'
}

try {
    if ($Mode -eq 'EnableOnly') {
        $dev = Enable-LightingIddDevice
        if ($dev) {
            Write-Result 'OK' ('ENABLED:' + $(if ($dev.InstanceId) { $dev.InstanceId } else { 'unknown' }))
            exit 0
        }
        Write-Result 'FAIL' 'DEVICE_NOT_FOUND'
        Write-Host 'FAIL|DEVICE_NOT_FOUND'
        exit 1
    }

    $dev = Enable-LightingIddDevice
    if (-not $dev) {
        Install-FromBundle -Dir $BundleDir
        $dev = Enable-LightingIddDevice
    }

    if (-not $dev) {
        Write-Result 'FAIL' 'DEVICE_STILL_MISSING'
        Write-Host 'FAIL|DEVICE_STILL_MISSING'
        Read-Host 'Lighting provision failed. Press Enter'
        exit 1
    }

    Write-Result 'OK' ('READY:' + $(if ($dev.InstanceId) { $dev.InstanceId } else { 'unknown' }))
    exit 0
} catch {
    $code = Map-ProvisionError $_.Exception.Message
    Write-Result 'FAIL' $code
    Write-Host ('FAIL|' + $code)
    Read-Host 'Lighting provision failed. Press Enter'
    exit 1
}
