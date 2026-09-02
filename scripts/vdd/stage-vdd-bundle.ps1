# Stage Virtual Display Driver files into host-ui/resources/vdd for portable bundling.
$ErrorActionPreference = 'Stop'

$outDir = Join-Path $PSScriptRoot '..\..\host-ui\resources\vdd'
if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
}
$outDir = (Resolve-Path $outDir).Path

Copy-Item (Join-Path $PSScriptRoot 'provision.ps1') (Join-Path $outDir 'provision.ps1') -Force

$NefConURL = 'https://github.com/nefarius/nefcon/releases/download/v1.14.0/nefcon_v1.14.0.zip'
$DriverURL = 'https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.7.23/VirtualDisplayDriver-x86.Driver.Only.zip'
$temp = Join-Path $env:TEMP ('LightingVDDStage-' + [guid]::NewGuid().ToString())
New-Item -ItemType Directory -Force -Path $temp | Out-Null

try {
    $nefZip = Join-Path $temp 'nefcon.zip'
    Invoke-WebRequest -Uri $NefConURL -OutFile $nefZip -UseBasicParsing
    Expand-Archive -Path $nefZip -DestinationPath $temp -Force
    $nef = Get-ChildItem -Path $temp -Recurse -Filter 'nefconc.exe' | Select-Object -First 1
    if (-not $nef) {
        $nef = Get-ChildItem -Path $temp -Recurse -Filter 'nefconw.exe' | Select-Object -First 1
    }
    if (-not $nef) { throw 'nefcon console/window helper not found in nefcon zip' }
    Copy-Item $nef.FullName (Join-Path $outDir $nef.Name) -Force
    $win = Get-ChildItem -Path $temp -Recurse -Filter 'nefconw.exe' | Select-Object -First 1
    if ($win -and $win.Name -ne $nef.Name) {
        Copy-Item $win.FullName (Join-Path $outDir $win.Name) -Force
    }

    $drvZip = Join-Path $temp 'driver.zip'
    Invoke-WebRequest -Uri $DriverURL -OutFile $drvZip -UseBasicParsing
    Expand-Archive -Path $drvZip -DestinationPath $temp -Force
    $inf = Get-ChildItem -Path $temp -Recurse -Filter 'MttVDD.inf' | Select-Object -First 1
    if (-not $inf) { throw 'MttVDD.inf not found in driver zip' }

    $srcDir = $inf.DirectoryName
    Get-ChildItem -Path $srcDir -File | ForEach-Object {
        Copy-Item $_.FullName (Join-Path $outDir $_.Name) -Force
    }

    Write-Host "VDD bundle staged at $outDir"
    Get-ChildItem $outDir | Format-Table Name, Length
} finally {
    Remove-Item -Path $temp -Recurse -Force -ErrorAction SilentlyContinue
}
