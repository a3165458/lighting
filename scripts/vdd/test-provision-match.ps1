# CI / local: staged INF is not a device; GlideX / generic names are not MttVDD.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$vdd = Join-Path $here 'provision.ps1'
$idd = Join-Path (Join-Path (Split-Path -Parent $here) 'idd') 'provision.ps1'
if (-not (Test-Path $vdd)) { throw "missing $vdd" }
if (-not (Test-Path $idd)) { throw "missing $idd" }

. $vdd
if (-not (Get-Command Test-IntendedVirtualHwid -ErrorAction SilentlyContinue)) {
    throw 'dotsourcing provision.ps1 did not export Test-IntendedVirtualHwid'
}

function Assert-True($cond, [string]$msg) {
    if (-not $cond) { throw $msg }
}
function Assert-False($cond, [string]$msg) {
    if ($cond) { throw $msg }
}

# pnputil 0/259 stages the store; nefcon must still run when no node exists.
Assert-True (Test-PnpSuccess 0) '0 is a store success'
Assert-True (Test-PnpSuccess 259) '259 is already-in-store'
Assert-False (Test-PnpSuccess 1) 'other codes are not store success'
Assert-True (Should-RunNefconInstall $false) 'QA oem90: staged INF still needs nefcon'
Assert-False (Should-RunNefconInstall $true) 'existing Root\MttVDD node does not need install'

$glidex = [pscustomobject]@{
    InstanceId = 'PCI\VEN_1002&DEV_1681'
    HardwareID = @('PCI\VEN_1002')
    FriendlyName = 'ASUS GlideX Display'
    Class = 'Display'
    Status = 'OK'
    ConfigManagerErrorCode = 0
}
Assert-False (Test-IntendedVirtualHwid -InstanceId $glidex.InstanceId -HardwareIds $glidex.HardwareID -FriendlyName $glidex.FriendlyName -Token 'MttVDD') 'GlideX adapter is not MttVDD'
Assert-False (Test-ReadyVirtualDevice -Device $glidex -Token 'MttVDD') 'GlideX is not a ready VDD'

$generic = [pscustomobject]@{
    InstanceId = 'DISPLAY\DEFAULT\1'
    HardwareID = @('MONITOR\DEFAULT')
    FriendlyName = 'Virtual Display'
    Class = 'Monitor'
    Status = 'OK'
    ConfigManagerErrorCode = 0
}
Assert-False (Test-IntendedVirtualHwid -InstanceId $generic.InstanceId -HardwareIds $generic.HardwareID -FriendlyName $generic.FriendlyName -Token 'MttVDD') 'generic Virtual Display name is not enough'
Assert-False (Test-ReadyVirtualDevice -Device $generic -Token 'MttVDD') 'generic name is not ready'

$mtt = [pscustomobject]@{
    InstanceId = 'ROOT\MTTVDD\0000'
    HardwareID = @('ROOT\MTTVDD', 'Root\MttVDD')
    FriendlyName = 'Virtual Display Driver'
    Class = 'Display'
    Status = 'OK'
    ConfigManagerErrorCode = 0
}
Assert-True (Test-IntendedVirtualHwid -InstanceId $mtt.InstanceId -HardwareIds $mtt.HardwareID -FriendlyName $mtt.FriendlyName -Token 'MttVDD') 'Root\MttVDD must match'
Assert-True (Test-ReadyVirtualDevice -Device $mtt -Token 'MttVDD') 'started MttVDD is ready'
Assert-False (Test-ReadyVirtualDevice -Device $mtt -Token 'LightingIdd') 'MttVDD is not LightingIdd'
Assert-True (Test-MttFamilyHwid -InstanceId $mtt.InstanceId -HardwareIds $mtt.HardwareID -FriendlyName $mtt.FriendlyName) 'Mtt family helper accepts Root\MttVDD'

$iddSample = [pscustomobject]@{
    InstanceId = 'ROOT\IDDSAMPLEDRIVER\0000'
    HardwareID = @('Root\IddSampleDriver')
    FriendlyName = 'IddSampleDriver Device HDR'
    Class = 'Display'
    Status = 'OK'
    ConfigManagerErrorCode = 0
}
Assert-True (Test-MttFamilyHwid -InstanceId $iddSample.InstanceId -HardwareIds $iddSample.HardwareID -FriendlyName $iddSample.FriendlyName) 'older Root\IddSampleDriver is the real VDD HWID'
Assert-False (Test-MttFamilyHwid -InstanceId $generic.InstanceId -HardwareIds $generic.HardwareID -FriendlyName $generic.FriendlyName) 'generic name is not Mtt family'

$problem = [pscustomobject]@{
    InstanceId = 'ROOT\MTTVDD\0000'
    HardwareID = @('ROOT\MTTVDD')
    FriendlyName = 'Virtual Display Driver'
    Class = 'Unknown'
    Status = 'Problem'
    ConfigManagerErrorCode = 28
}
Assert-True (Test-IntendedVirtualHwid -InstanceId $problem.InstanceId -HardwareIds $problem.HardwareID -FriendlyName $problem.FriendlyName -Token 'MttVDD') 'problem node still has the HWID'
Assert-False (Test-ReadyVirtualDevice -Device $problem -Token 'MttVDD') 'unknown-class problem node is not ready'

# Idd script uses the same helpers.
. $idd
$iddDev = [pscustomobject]@{
    InstanceId = 'ROOT\LIGHTINGIDD\0000'
    HardwareID = @('Root\LightingIdd')
    FriendlyName = 'Lighting Virtual Display'
    Class = 'System'
    Status = 'OK'
    ConfigManagerErrorCode = 0
}
Assert-True (Test-IntendedVirtualHwid -InstanceId $iddDev.InstanceId -HardwareIds $iddDev.HardwareID -FriendlyName $iddDev.FriendlyName -Token 'LightingIdd') 'Root\LightingIdd must match'
Assert-True (Test-ReadyVirtualDevice -Device $iddDev -Token 'LightingIdd') 'started LightingIdd is ready'
Assert-False (Test-IntendedVirtualHwid -InstanceId $glidex.InstanceId -HardwareIds $glidex.HardwareID -FriendlyName $glidex.FriendlyName -Token 'LightingIdd') 'GlideX is not LightingIdd'
Assert-True (Should-RunNefconInstall $false) 'Idd also creates the node after staging'

Write-Host 'OK: provision match rejects GlideX/generic names; staged INF still needs nefcon'
