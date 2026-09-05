# Provision the signed MttVDD root device. Rust owns display topology and primary-screen preservation.
# Writes OK|READY:<instance> or FAIL|<code> to -ResultFile; never treats driver-store staging as a device.
param(
    [Parameter(Mandatory = $true)]
    [string]$BundleDir,
    [Parameter(Mandatory = $true)]
    [string]$ResultFile,
    [ValidateSet('Full', 'EnableOnly')]
    [string]$Mode = 'Full'
)

$ErrorActionPreference = 'Stop'
$VddReg = 'HKLM:\SOFTWARE\MikeTheTech\VirtualDisplayDriver'
$script:RestartNeeded = $false

function Write-Result([string]$Status, [string]$Detail) {
    $safe = $Detail -replace '[^a-zA-Z0-9_\-:.]', '_'
    if ($safe.Length -gt 120) { $safe = $safe.Substring(0, 120) }
    $parent = Split-Path -Parent $ResultFile
    if ($parent -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    Set-Content -LiteralPath $ResultFile -Value ($Status + '|' + $safe) -Encoding ASCII -NoNewline
}

function Find-MttDevice {
    # A newly-created, unbound root node might not have a Display class yet.
    # HardwareID is stable; nefcon-created instance IDs are usually ROOT\DISPLAY\000x.
    $devices = @(Get-PnpDevice -PresentOnly -ErrorAction Stop | Where-Object {
        ($_.HardwareID -contains 'Root\MttVDD') -or ($_.InstanceId -like 'ROOT\MTTVDD\*')
    })
    if ($devices.Count -gt 1) { throw 'DEVICE_DUPLICATES:Remove_extra_MttVDD_adapters_in_Device_Manager' }
    return $devices | Select-Object -First 1
}

function Ensure-VddSettings {
    $configured = Get-ItemProperty -Path $VddReg -Name VDDPATH -ErrorAction SilentlyContinue
    $dir = if ($configured -and $configured.VDDPATH) { [string]$configured.VDDPATH } else { 'C:\VirtualDisplayDriver' }
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $xmlPath = Join-Path $dir 'vdd_settings.xml'
    if (-not (Test-Path -LiteralPath $xmlPath)) {
        $defaults = Join-Path $BundleDir 'vdd_settings.xml'
        if (-not (Test-Path -LiteralPath $defaults)) { throw 'BUNDLE_SETTINGS_MISSING' }
        Copy-Item -LiteralPath $defaults -Destination $xmlPath
        $script:RestartNeeded = $true
    }
    # Preserve user resolutions, GPU choice, EDID and all other settings.
    $xml = New-Object System.Xml.XmlDocument
    $xml.PreserveWhitespace = $true
    $xml.Load($xmlPath)
    $count = $xml.SelectSingleNode('/vdd_settings/monitors/count')
    if (-not $count) { throw 'SETTINGS_MONITOR_COUNT_MISSING' }
    $number = 0
    if (-not [int]::TryParse($count.InnerText, [ref]$number) -or $number -lt 0) {
        throw 'SETTINGS_MONITOR_COUNT_INVALID'
    }
    if ($number -eq 0) {
        Copy-Item -LiteralPath $xmlPath -Destination ($xmlPath + '.lighting-backup-' + [guid]::NewGuid().ToString('N'))
        $count.InnerText = '1'
        $xml.Save($xmlPath)
        $script:RestartNeeded = $true
    }
    if (-not $configured -or -not $configured.VDDPATH) {
        New-Item -Path $VddReg -Force | Out-Null
        New-ItemProperty -Path $VddReg -Name VDDPATH -Value $dir -PropertyType String -Force | Out-Null
    }
}

function Assert-NativeSuccess([int]$Code, [string]$Operation) {
    if ($Code -eq 3010 -or $Code -eq 1641) { throw 'REBOOT_REQUIRED:Restart_Windows_then_retry' }
    if ($Code -ne 0) { throw ($Operation + ':exit_' + $Code) }
}

function Install-FromBundle($Device) {
    # Pins match the official 25.7.23 driver-only and nefcon v1.14.0 x64 packages.
    $hashes = @{
        'MttVDD.inf' = '550d211fe481e74dfe3f9d724ed78be48b3a9113405965d683d9373e8d672f5d'
        'MttVDD.dll' = 'c9ca837f57a98fbd43bc416a7f535a95843626e7759eaf85cf0cd7ce334dbb05'
        'mttvdd.cat' = '08a0093fc9b2e32b287a6f8a77ca4de0a31830d29fc33d2b13a918dc859468f6'
        'nefconc.exe' = '99ed0d588a1eb7c4306ac59aa5bc47f7458fb70d2870957829962203c7fb989d'
    }
    foreach ($entry in $hashes.GetEnumerator()) {
        $file = Join-Path $BundleDir $entry.Key
        if (-not (Test-Path -LiteralPath $file)) { throw ('BUNDLE_FILE_MISSING:' + $entry.Key) }
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $entry.Value) {
            throw ('BUNDLE_HASH_MISMATCH:' + $entry.Key)
        }
    }
    $signature = Get-AuthenticodeSignature -LiteralPath (Join-Path $BundleDir 'mttvdd.cat')
    if ($signature.Status -ne 'Valid' -or -not $signature.SignerCertificate) {
        throw ('DRIVER_SIGNATURE:' + $signature.Status)
    }
    # Trust only the verified catalog publisher, never add certificates to Root or change Secure Boot.
    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store('TrustedPublisher', 'LocalMachine')
    try {
        $store.Open('ReadWrite')
        $store.Add($signature.SignerCertificate)
    } finally {
        $store.Close()
    }
    $inf = Join-Path $BundleDir 'MttVDD.inf'
    if ($Device) {
        # Repair binding on an existing node without creating a duplicate adapter.
        & "$env:SystemRoot\System32\pnputil.exe" /add-driver $inf /install
        Assert-NativeSuccess $LASTEXITCODE 'DRIVER_BIND_FAILED'
    } else {
        # pnputil /add-driver alone does NOT create a root-enumerated virtual display.
        # nefcon v1.14.0 explicitly supports this devcon-compatible install form.
        & (Join-Path $BundleDir 'nefconc.exe') install $inf 'Root\MttVDD'
        Assert-NativeSuccess $LASTEXITCODE 'DRIVER_INSTALL_FAILED'
    }
}

try {
    if (-not [Environment]::Is64BitProcess -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') {
        throw 'UNSUPPORTED_ARCHITECTURE:Use_x64_Windows_PowerShell'
    }
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'ACCESS_DENIED:Driver_provisioning_requires_elevation'
    }
    $BundleDir = (Resolve-Path -LiteralPath $BundleDir).Path
    Ensure-VddSettings
    $device = Find-MttDevice
    if (-not $device -and $Mode -eq 'EnableOnly') { throw 'DEVICE_NOT_FOUND' }
    if ($Mode -eq 'Full' -and (-not $device -or $device.Status -ne 'OK')) {
        Install-FromBundle $device
    }
    $device = Find-MttDevice
    if (-not $device) { throw 'DEVICE_STILL_MISSING' }
    $problem = Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName 'DEVPKEY_Device_ProblemCode'
    if ($problem.Data -eq 22) {
        Enable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false -ErrorAction Stop
    }
    if ($script:RestartNeeded) {
        & "$env:SystemRoot\System32\pnputil.exe" /restart-device $device.InstanceId
        Assert-NativeSuccess $LASTEXITCODE 'DEVICE_RESTART_FAILED'
    }
    for ($attempt = 0; $attempt -lt 20; $attempt++) {
        $device = Find-MttDevice
        if ($device -and $device.Status -eq 'OK') {
            $problem = Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName 'DEVPKEY_Device_ProblemCode'
            if ($problem.Data -eq 0) {
                Write-Result 'OK' ('READY:' + $device.InstanceId)
                exit 0
            }
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not $device) { throw 'DEVICE_STILL_MISSING' }
    $problem = Get-PnpDeviceProperty -InstanceId $device.InstanceId -KeyName 'DEVPKEY_Device_ProblemCode'
    throw ('DEVICE_NOT_READY:problem_' + $problem.Data + ':status_' + $device.Status)
} catch {
    Write-Result 'FAIL' $_.Exception.Message
    Write-Error -Message $_.Exception.Message -ErrorAction Continue
    exit 1
}
