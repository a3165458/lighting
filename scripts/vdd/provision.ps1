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
    $safe = ($Detail -replace '[^a-zA-Z0-9_\-:.]', '_')
    if ($safe.Length -gt 120) { $safe = $safe.Substring(0, 120) }
    $line = "$Status|$safe"
    $parent = Split-Path -Parent $ResultFile
    if ($parent -and -not (Test-Path $parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
    Set-Content -Path $ResultFile -Value $line -Encoding ASCII -NoNewline
}

function Find-MttDevice {
    try {
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
    } catch {
        return $null
    }
}

function Enable-MttDevice {
    $device = Find-MttDevice
    if (-not $device) {
        try { pnputil /enable-device /deviceid 'Root\MttVDD' 2>&1 | Out-Null } catch {}
        $device = Find-MttDevice
    }
    if (-not $device) { return $null }
    try { Enable-PnpDevice -InstanceId $device.InstanceId -Confirm:$false -ErrorAction SilentlyContinue } catch {}
    try { pnputil /enable-device $device.InstanceId 2>&1 | Out-Null } catch {}
    try { pnputil /restart-device $device.InstanceId 2>&1 | Out-Null } catch {}
    Start-Sleep -Seconds 2
    return (Find-MttDevice)
}

function Test-PnpSuccess([int]$ExitCode) {
    # 0 = ok; 259 (0x103) = driver already in driver store
    return ($ExitCode -eq 0 -or $ExitCode -eq 259)
}

function Install-FromBundle([string]$Dir) {
    if (-not (Test-Path $Dir)) { throw 'BUNDLE_DIR_MISSING' }

    $inf = Get-ChildItem -Path $Dir -Recurse -Filter 'MttVDD.inf' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $inf) { throw 'BUNDLE_INF_MISSING' }

    $infDir = $inf.DirectoryName
    $nef = Get-ChildItem -Path $Dir -Recurse -Filter 'nefconw.exe' -ErrorAction SilentlyContinue | Select-Object -First 1

    $cat = Get-ChildItem -Path $infDir -Filter '*.cat' -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cat) {
        try {
            $certs = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2Collection
            $certs.Import([IO.File]::ReadAllBytes($cat.FullName))
            foreach ($cert in $certs) {
                $cer = Join-Path $env:TEMP ($cert.Thumbprint + '.cer')
                [IO.File]::WriteAllBytes($cer, $cert.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert))
                Import-Certificate -FilePath $cer -CertStoreLocation 'Cert:\LocalMachine\TrustedPublisher' -ErrorAction SilentlyContinue | Out-Null
            }
        } catch {}
    }

    $added = $false
    $pnputilEc = -1
    try {
        pnputil /add-driver $inf.FullName /install 2>&1 | Out-Null
        $pnputilEc = $LASTEXITCODE
        if (Test-PnpSuccess $pnputilEc) { $added = $true }
    } catch {
        $pnputilEc = $LASTEXITCODE
    }

    $nefEc = -1
    if (-not $added -and $nef) {
        Push-Location $infDir
        try {
            & $nef.FullName install $inf.Name 'Root\MttVDD' 2>&1 | Out-Null
            $nefEc = $LASTEXITCODE
            if (Test-PnpSuccess $nefEc) { $added = $true }
        } catch {
            $nefEc = $LASTEXITCODE
        } finally {
            Pop-Location
        }
    }

    if (-not $added) {
        if (-not $nef) {
            throw "DRIVER_INSTALL_FAILED:pnputil=$pnputilEc"
        }
        throw "DRIVER_INSTALL_FAILED:pnputil=$pnputilEc,nefcon=$nefEc"
    }
    Start-Sleep -Seconds 6
}

function Map-ProvisionError([string]$Message) {
    $m = $Message
    if ($m -match 'BUNDLE_INF_MISSING|BUNDLE_DIR_MISSING|DRIVER_INSTALL_FAILED|DEVICE') {
        return ($Matches[0])
    }
    if ($m -match 'access|denied|authorized|elevation|administrator|0x5|5\)|1326') {
        return 'ACCESS_DENIED'
    }
    if ($m -match 'sign|certificate|catalog|trust|blocked') {
        return 'DRIVER_SIGNATURE'
    }
    if ($m -match 'PnP|Get-PnpDevice|CIM|Win32') {
        return 'PNP_QUERY_FAILED'
    }
    $slug = ($m -replace '[^a-zA-Z0-9_ ]', '') -replace '\s+', '_'
    if ($slug.Length -gt 48) { $slug = $slug.Substring(0, 48) }
    if ([string]::IsNullOrWhiteSpace($slug)) { return 'UNEXPECTED' }
    return "ERR_$slug"
}

try {
    if ($Mode -eq 'EnableOnly') {
        $dev = Enable-MttDevice
        if ($dev) {
            $id = if ($dev.InstanceId) { $dev.InstanceId } else { 'unknown' }
            Write-Result 'OK' ('ENABLED:' + $id)
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

    $id = if ($dev.InstanceId) { $dev.InstanceId } else { 'unknown' }
    Write-Result 'OK' ('READY:' + $id)
    exit 0
} catch {
    $code = Map-ProvisionError $_.Exception.Message
    Write-Result 'FAIL' $code
    exit 1
}
