# Lighting bundled virtual display provisioning (ASCII output only).
# Writes "<STATUS>|<DETAIL>" to -ResultFile. STATUS is OK or FAIL.
param(
    [Parameter(Mandatory = $true)]
    [string]$BundleDir,
    [Parameter(Mandatory = $true)]
    [string]$ResultFile,
    [ValidateSet('Full', 'EnableOnly')]
    [string]$Mode = 'Full'
)

$ErrorActionPreference = 'Stop'

function Write-Result([string]$Status, [string]$Detail) {
    $line = "$Status|$Detail"
    Set-Content -Path $ResultFile -Value $line -Encoding ASCII -NoNewline
}

function Find-MttDevice {
    $names = @(
        'Virtual Display Driver',
        'IddSampleDriver Device HDR',
        'MttVDD Display Adapter',
        'MttVDD'
    )
    $device = Get-PnpDevice -Class Display -ErrorAction SilentlyContinue | Where-Object {
        ($names -contains $_.FriendlyName) -or
        ($_.InstanceId -like '*MttVDD*') -or
        ($_.HardwareID -like '*MttVDD*')
    } | Select-Object -First 1
    if (-not $device) {
        $device = Get-PnpDevice -HardwareID 'Root\MttVDD' -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    if (-not $device) {
        $device = Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
            ($_.InstanceId -like '*MttVDD*') -or
            (($_.FriendlyName) -and ($_.FriendlyName -match 'Virtual Display|MttVDD|IddSample'))
        } | Select-Object -First 1
    }
    return $device
}

function Enable-MttDevice {
    $device = Find-MttDevice
    if (-not $device) {
        try { pnputil /enable-device /deviceid 'Root\MttVDD' | Out-Null } catch {}
        $device = Find-MttDevice
    }
    if (-not $device) { return $null }
    try { Enable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false -ErrorAction SilentlyContinue } catch {}
    try { pnputil /enable-device $device.InstanceId | Out-Null } catch {}
    try { pnputil /restart-device $device.InstanceId | Out-Null } catch {}
    Start-Sleep -Seconds 2
    return (Find-MttDevice)
}

function Install-FromBundle([string]$Dir) {
    $inf = Get-ChildItem -Path $Dir -Recurse -Filter 'MttVDD.inf' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $inf) { throw 'BUNDLE_INF_MISSING' }

    $infDir = $inf.DirectoryName
    $nef = Get-ChildItem -Path $Dir -Recurse -Filter 'nefconw.exe' -ErrorAction SilentlyContinue | Select-Object -First 1

    # Trust publisher certs when a catalog is shipped with the bundle.
    $cat = Get-ChildItem -Path $infDir -Filter '*.cat' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cat) {
        try {
            $certs = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2Collection
            $certs.Import([IO.File]::ReadAllBytes($cat.FullName))
            foreach ($cert in $certs) {
                $cer = Join-Path $env:TEMP ($cert.Thumbprint + '.cer')
                [IO.File]::WriteAllBytes($cer, $cert.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert))
                Import-Certificate -FilePath $cer -CertStoreLocation 'Cert:\LocalMachine\TrustedPublisher' | Out-Null
            }
        } catch {}
    }

    $added = $false
    try {
        pnputil /add-driver $inf.FullName /install | Out-Null
        if ($LASTEXITCODE -eq 0) { $added = $true }
    } catch {}

    if (-not $added -and $nef) {
        Push-Location $infDir
        try {
            & $nef.FullName install $inf.Name 'Root\MttVDD'
            if ($LASTEXITCODE -eq 0) { $added = $true }
        } finally {
            Pop-Location
        }
    }

    if (-not $added) { throw 'DRIVER_INSTALL_FAILED' }
    Start-Sleep -Seconds 6
}

try {
    if ($Mode -eq 'EnableOnly') {
        $dev = Enable-MttDevice
        if ($dev) {
            Write-Result 'OK' ('ENABLED:' + $dev.InstanceId)
            exit 0
        }
        Write-Result 'FAIL' 'DEVICE_NOT_FOUND'
        exit 1
    }

    $dev = Enable-MttDevice
    if (-not $dev) {
        Install-FromBundle -Dir $BundleDir
        $dev = Enable-MttDevice
    }

    if (-not $dev) {
        Write-Result 'FAIL' 'DEVICE_STILL_MISSING'
        exit 1
    }

    Write-Result 'OK' ('READY:' + $dev.InstanceId)
    exit 0
} catch {
    $msg = $_.Exception.Message
    if ($msg -match 'BUNDLE_INF_MISSING|DRIVER_INSTALL_FAILED|DEVICE') {
        Write-Result 'FAIL' $msg
    } else {
        Write-Result 'FAIL' 'UNEXPECTED'
    }
    exit 1
}
