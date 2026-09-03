# Build LightingIdd.dll (UMDF IddCx) with VS + WDK when available.
# Exit 0 only when LightingIdd.dll is produced. Missing WDK is a hard failure
# so pack jobs do not silently ship INF-only Idd.
param(
    [string]$RepoRoot = '',
    [string]$Configuration = 'Release',
    [string]$Platform = 'x64'
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
}

$sln = Join-Path $RepoRoot 'driver-idd\LightingIdd.sln'
if (-not (Test-Path $sln)) {
    throw "missing $sln"
}

function Find-MSBuild {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $vswhere) {
        $hint = & $vswhere -latest -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe' |
            Select-Object -First 1
        if ($hint -and (Test-Path $hint)) {
            return $hint
        }
    }
    $cmd = Get-Command msbuild -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    return $null
}

function Find-BuiltDll([string]$Root) {
    $paths = @(
        (Join-Path $Root "driver-idd\$Platform\$Configuration\LightingIdd.dll"),
        (Join-Path $Root "driver-idd\LightingIdd\$Platform\$Configuration\LightingIdd.dll"),
        (Join-Path $Root "driver-idd\$Configuration\LightingIdd.dll"),
        (Join-Path $Root "driver-idd\src\$Platform\$Configuration\LightingIdd.dll")
    )
    foreach ($p in $paths) {
        if (Test-Path $p) { return $p }
    }
    $found = Get-ChildItem -Path (Join-Path $Root 'driver-idd') -Recurse -Filter 'LightingIdd.dll' -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($found) { return $found.FullName }
    return $null
}

$existing = Find-BuiltDll $RepoRoot
if ($existing) {
    Write-Host "LightingIdd.dll already present: $existing"
    exit 0
}

$msbuild = Find-MSBuild
if (-not $msbuild) {
    throw 'MSBuild not found. Install Visual Studio 2022 C++ workload + WDK, or set LIGHTING_IDD_DLL to a prebuilt LightingIdd.dll.'
}
Write-Host "MSBuild: $msbuild"

$packagesConfig = Join-Path $RepoRoot 'driver-idd\packages.config'
$packagesDir = Join-Path $RepoRoot 'driver-idd\packages'
if (Test-Path $packagesConfig) {
    $nuget = Get-Command nuget -ErrorAction SilentlyContinue
    if (-not $nuget) {
        $nugetExe = Join-Path $env:TEMP 'nuget.exe'
        if (-not (Test-Path $nugetExe)) {
            Write-Host 'Downloading nuget.exe'
            Invoke-WebRequest -Uri 'https://dist.nuget.org/win-x86-commandline/latest/nuget.exe' -OutFile $nugetExe -UseBasicParsing
        }
        $nuget = $nugetExe
    } else {
        $nuget = $nuget.Source
    }
    Write-Host "Restoring WDK NuGet packages"
    & $nuget restore $packagesConfig -PackagesDirectory $packagesDir
    if ($LASTEXITCODE -ne 0) {
        throw "nuget restore failed with exit $LASTEXITCODE"
    }
}

$msbuildArgs = @(
    $sln,
    '/m',
    "/p:Configuration=$Configuration",
    "/p:Platform=$Platform",
    '/p:RunCodeAnalysis=false',
    '/p:Driver_SpectreMitigation=false',
    '/p:InfVerif=false',
    '/p:TreatWarningAsError=false'
)

# Inbox WindowsUserModeDriver10.0 compiles, but its kit tree often lacks UMDF 2.25.
# Directory.Build.props appends include/lib dirs from this NuGet package — do not
# set WDKContentRoot (that loads NuGet targets and hits MSB4086 on MSBuild 17.14).
$wdkHeader = $null
if (Test-Path $packagesDir) {
    $wdkHeader = Get-ChildItem -Path $packagesDir -Directory -Filter 'Microsoft.Windows.WDK.x64.*' -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName 'c\Include\wdf\umdf\2.25\wudfwdm.h' } |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1
}
if ($wdkHeader) {
    Write-Host "UMDF headers: $wdkHeader"
} else {
    Write-Warning 'NuGet WDK headers (wudfwdm.h) not found under driver-idd/packages — MSBuild may fail on UMDF includes.'
}

Write-Host "Building LightingIdd ($Configuration|$Platform)"
& $msbuild @msbuildArgs
if ($LASTEXITCODE -ne 0) {
    throw "MSBuild failed with exit $LASTEXITCODE"
}

$dll = Find-BuiltDll $RepoRoot
if (-not $dll) {
    throw 'MSBuild reported success but LightingIdd.dll was not found under driver-idd/'
}
Write-Host "Built $dll"
exit 0
