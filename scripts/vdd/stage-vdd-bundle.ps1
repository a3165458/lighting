# Stage the official signed x64 driver and root-device installer for offline provisioning.
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$outDir = Join-Path $PSScriptRoot '..\..\host-ui\resources\vdd'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$outDir = (Resolve-Path $outDir).Path
$temp = Join-Path $env:TEMP ('LightingVDDStage-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temp | Out-Null

function Get-VerifiedArchive([string]$Url, [string]$Hash, [string]$Name) {
    $zip = Join-Path $temp ($Name + '.zip')
    Invoke-WebRequest -Uri $Url -OutFile $zip -UseBasicParsing
    if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash -ne $Hash) {
        throw "SHA256 mismatch for $Name"
    }
    $dir = Join-Path $temp $Name
    Expand-Archive -LiteralPath $zip -DestinationPath $dir
    return $dir
}

try {
    $nefDir = Get-VerifiedArchive `
        'https://github.com/nefarius/nefcon/releases/download/v1.14.0/nefcon_v1.14.0.zip' `
        'a15557da24a9efca203158de3b43b0eaf982db231f0194031f1ed428bc13e669' 'nefcon'
    $driverDir = Get-VerifiedArchive `
        'https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualDisplayDriver-x86.Driver.Only.zip' `
        'e24210692b442b39af763536330ce78b423f19342b7a7792c26de3944e418b3a' 'driver'
    # Despite the upstream asset's x86 name, its INF targets NTamd64.
    # The nefcon archive starts with ARM64: never select the first recursive match.
    Copy-Item -LiteralPath (Join-Path $nefDir 'x64\nefconc.exe') -Destination (Join-Path $outDir 'nefconc.exe') -Force
    Copy-Item -LiteralPath (Join-Path $nefDir 'x64\nefconw.exe') -Destination (Join-Path $outDir 'nefconw.exe') -Force
    foreach ($name in @('MttVDD.inf', 'MttVDD.dll', 'mttvdd.cat', 'vdd_settings.xml')) {
        Copy-Item -LiteralPath (Join-Path $driverDir "VirtualDisplayDriver\$name") -Destination (Join-Path $outDir $name) -Force
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'provision.ps1') -Destination (Join-Path $outDir 'provision.ps1') -Force
    # Keep the upstream licenses with the redistributed binary tools.
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/nefarius/nefcon/v1.14.0/LICENSE' -OutFile (Join-Path $outDir 'LICENSE-nefcon') -UseBasicParsing
    Invoke-WebRequest -Uri 'https://raw.githubusercontent.com/VirtualDrivers/Virtual-Display-Driver/25.7.23/LICENSE' -OutFile (Join-Path $outDir 'LICENSE-Virtual-Display-Driver') -UseBasicParsing
    Write-Host "Signed x64 VDD bundle staged at $outDir"
} finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
