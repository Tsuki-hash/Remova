#!/usr/bin/env pwsh
# Install / upgrade / rollback / uninstall matrix for the per-user NSIS package.
#
# Destructive by design: it replaces the Remova installation on THIS machine.
# It therefore refuses to run without -ConfirmReplaceInstalledApp, backs up the
# installed directory, the per-user config and the uninstall registry key first,
# and restores the original installation in `finally` — including after a failed
# assertion.
#
# Every step is recorded as one JSON record (step, ok, detail, at). The exit code
# is non-zero if any assertion failed.
param(
    [Parameter(Mandatory = $true)][string]$Installer140,
    [Parameter(Mandatory = $true)][string]$Installer141,
    [string]$InstallDir = 'D:\Users\Aaron\AppData\Local\Remova',
    [string]$ConfigDir = '',
    [string]$Out = '',
    [switch]$ConfirmReplaceInstalledApp
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ([string]::IsNullOrWhiteSpace($ConfigDir)) {
    $ConfigDir = Join-Path $env:LOCALAPPDATA 'Remova'
}
if ([string]::IsNullOrWhiteSpace($Out)) {
    $Out = Join-Path (Join-Path $repoRoot 'benchmarks') ("runs/install-matrix-" + (Get-Date).ToString('yyyyMMdd-HHmmss'))
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null

if (-not $ConfirmReplaceInstalledApp) {
    throw 'Refusing to replace the installed application without -ConfirmReplaceInstalledApp.'
}
foreach ($exe in @($Installer140, $Installer141)) {
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Installer not found: $exe" }
}
if ((Get-Process Remova -ErrorAction SilentlyContinue)) {
    throw 'A Remova process is running; close it before running the matrix.'
}

$records = [System.Collections.Generic.List[object]]::new()
function Record([string]$step, [bool]$ok, [string]$detail) {
    $records.Add([ordered]@{ step = $step; ok = $ok; detail = $detail; at = (Get-Date).ToString('o') })
    $colour = if ($ok) { 'Green' } else { 'Red' }
    Write-Host ("[{0}] {1}: {2}" -f $(if ($ok) { 'ok' } else { 'FAIL' }), $step, $detail) -ForegroundColor $colour
}
function Assert([string]$step, [bool]$condition, [string]$detail) {
    if (-not $condition) { Record $step $false $detail; throw "$step failed: $detail" }
    Record $step $true $detail
}

function Get-InstalledVersion {
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Remova'
    (Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue).DisplayVersion
}
function Get-InstalledLocation {
    $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Remova'
    (Get-ItemProperty -LiteralPath $key -ErrorAction SilentlyContinue).InstallLocation
}
function Get-ExeVersion([string]$dir) {
    $exe = Join-Path $dir 'remova.exe'
    if (-not (Test-Path -LiteralPath $exe)) { return $null }
    (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
}
function Install-Silent([string]$exe, [string]$dir) {
    # NSIS: /S is silent; /D= must be the last argument and unquoted.
    $arguments = @('/S')
    if (-not [string]::IsNullOrWhiteSpace($dir)) { $arguments += "/D=$dir" }
    $process = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru -Wait
    $process.ExitCode
}
function Wait-For([scriptblock]$probe, [int]$seconds = 30) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Get-Date) -lt $deadline) {
        if (& $probe) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}
function Uninstall-Silent([string]$dir) {
    $uninstaller = Join-Path $dir 'uninstall.exe'
    if (-not (Test-Path -LiteralPath $uninstaller)) { return }
    Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru -Wait | Out-Null
    Wait-For { -not (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Remova') } 60 | Out-Null
}
function Start-Check([string]$dir) {
    $exe = Join-Path $dir 'remova.exe'
    $proc = Start-Process -FilePath $exe -PassThru
    try {
        Start-Sleep -Seconds 4
        $proc.Refresh()
        if ($proc.HasExited) { return $false }
        return (Get-Process -Name Remova -ErrorAction SilentlyContinue) -ne $null
    } finally {
        if (-not $proc.HasExited) { $proc.Kill(); $proc.WaitForExit(10000) | Out-Null }
        Get-Process -Name Remova -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
}

$backup = Join-Path $Out 'backup'
New-Item -ItemType Directory -Force -Path $backup | Out-Null
$originalVersion = Get-InstalledVersion
Record 'preflight' $true ("installed={0}; location={1}" -f $originalVersion, (Get-InstalledLocation))
if (Test-Path -LiteralPath $InstallDir) {
    Copy-Item -LiteralPath $InstallDir -Destination (Join-Path $backup 'app') -Recurse -Force
}
if (Test-Path -LiteralPath $ConfigDir) {
    Copy-Item -LiteralPath $ConfigDir -Destination (Join-Path $backup 'config') -Recurse -Force
}
$regBackup = Join-Path $backup 'uninstall-key.reg'
& reg.exe export 'HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Remova' $regBackup /y | Out-Null
Assert 'backup' ((Test-Path (Join-Path $backup 'app')) -and (Test-Path $regBackup)) "backup at $backup"

$configBefore = if (Test-Path (Join-Path $ConfigDir 'ai-config.json')) {
    (Get-FileHash -LiteralPath (Join-Path $ConfigDir 'ai-config.json') -Algorithm SHA256).Hash
} else { $null }

try {
    # 1) Install the older release.
    $code = Install-Silent $Installer140 $InstallDir
    Assert 'install-1.4.0' ($code -eq 0) "exit=$code"
    Assert 'install-1.4.0-version' ((Wait-For { (Get-InstalledVersion) -eq '1.4.0' }) -and (Get-InstalledVersion) -eq '1.4.0') ("registry={0}; exe={1}" -f (Get-InstalledVersion), (Get-ExeVersion $InstallDir))
    Assert 'install-1.4.0-location' ((Get-InstalledLocation) -like "*$InstallDir*") ("location={0}" -f (Get-InstalledLocation))
    Assert 'install-1.4.0-launches' (Start-Check $InstallDir) 'process started and stayed alive for 4s'

    # 2) Upgrade over it.
    $code = Install-Silent $Installer141 $InstallDir
    Assert 'upgrade-1.4.1' ($code -eq 0) "exit=$code"
    Assert 'upgrade-1.4.1-version' ((Wait-For { (Get-InstalledVersion) -eq '1.4.1' }) -and (Get-InstalledVersion) -eq '1.4.1') ("registry={0}; exe={1}" -f (Get-InstalledVersion), (Get-ExeVersion $InstallDir))
    $entries = @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' |
        ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -like '*Remova*' })
    Assert 'upgrade-single-entry' ($entries.Count -eq 1) ("uninstall entries={0}" -f $entries.Count)
    if ($configBefore) {
        $configAfter = (Get-FileHash -LiteralPath (Join-Path $ConfigDir 'ai-config.json') -Algorithm SHA256).Hash
        Assert 'upgrade-keeps-config' ($configAfter -eq $configBefore) 'ai-config.json unchanged by the upgrade'
    }

    # 3) Roll back to the older release (downgrade over the newer install).
    $code = Install-Silent $Installer140 $InstallDir
    Assert 'rollback-1.4.0' ($code -eq 0) "exit=$code"
    Assert 'rollback-1.4.0-version' ((Wait-For { (Get-InstalledVersion) -eq '1.4.0' }) -and (Get-InstalledVersion) -eq '1.4.0') ("registry={0}; exe={1}" -f (Get-InstalledVersion), (Get-ExeVersion $InstallDir))
    Assert 'rollback-1.4.0-launches' (Start-Check $InstallDir) 'process started and stayed alive for 4s'

    # 4) Uninstall.
    Uninstall-Silent $InstallDir
    Assert 'uninstall-key-removed' ($null -eq (Get-InstalledVersion)) ("registry={0}" -f (Get-InstalledVersion))
    # The uninstaller must remove what it installed. Runtime data the
    # application created (channel marker, generated icon) may remain, so it is
    # listed rather than treated as a failure.
    $leftoverNames = @(Get-ChildItem -LiteralPath $InstallDir -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
    $installedBinaries = @('remova.exe', 'list_apps.exe', 'uninstall.exe', 'verify_update.exe')
    $binariesLeft = @($leftoverNames | Where-Object { $installedBinaries -contains $_ })
    Assert 'uninstall-binaries-removed' ($binariesLeft.Count -eq 0) ("leftover binaries={0}" -f ($binariesLeft -join ','))
    Record 'uninstall-leftovers' $true ("remaining runtime files={0}" -f ($leftoverNames -join ','))
} finally {
    # Restore the original installation no matter where the matrix stopped.
    Uninstall-Silent $InstallDir
    # If an installer ignored /D=, it landed in the default per-user location,
    # which is also this app's config directory. Remove only the binaries there;
    # the config files are restored from the backup below.
    $defaultDir = Join-Path $env:LOCALAPPDATA 'Remova'
    if ((Test-Path (Join-Path $defaultDir 'remova.exe')) -and ($defaultDir -ne $InstallDir)) {
        Uninstall-Silent $defaultDir
        foreach ($name in @('remova.exe', 'list_apps.exe', 'uninstall.exe', 'verify_update.exe', '.remova-install-channel')) {
            Remove-Item -LiteralPath (Join-Path $defaultDir $name) -Force -ErrorAction SilentlyContinue
        }
        Get-ChildItem -LiteralPath $defaultDir -Filter 'remova-icon-*.ico' -ErrorAction SilentlyContinue |
            Remove-Item -Force -ErrorAction SilentlyContinue
        Record 'cleanup-default-location' $true "removed binaries from $defaultDir"
    }
    if ($originalVersion) {
        $code = Install-Silent $Installer141 $InstallDir
        Wait-For { (Get-InstalledVersion) -eq $originalVersion } 60 | Out-Null
        Record 'restore-install' ((Get-InstalledVersion) -eq $originalVersion) ("exit=$code; version={0}" -f (Get-InstalledVersion))
    }
    if (Test-Path (Join-Path $backup 'config')) {
        Get-ChildItem -LiteralPath (Join-Path $backup 'config') -Force | ForEach-Object {
            Copy-Item -LiteralPath $_.FullName -Destination $ConfigDir -Recurse -Force
        }
        Record 'restore-config' $true "restored $ConfigDir from $backup"
    }
    if (Test-Path (Join-Path $backup 'app\.remova-install-channel')) {
        Copy-Item -LiteralPath (Join-Path $backup 'app\.remova-install-channel') -Destination $InstallDir -Force
    }
    if ($configBefore) {
        $now = (Get-FileHash -LiteralPath (Join-Path $ConfigDir 'ai-config.json') -Algorithm SHA256).Hash
        Record 'restore-config-verify' ($now -eq $configBefore) 'ai-config.json hash matches the pre-run value'
    }
}

$result = [ordered]@{
    kind         = 'install-matrix'
    generated_at = (Get-Date).ToString('o')
    install_dir  = $InstallDir
    config_dir   = $ConfigDir
    installers   = [ordered]@{
        v140 = [ordered]@{ path = (Resolve-Path $Installer140).Path; sha256 = (Get-FileHash -LiteralPath $Installer140 -Algorithm SHA256).Hash.ToLowerInvariant() }
        v141 = [ordered]@{ path = (Resolve-Path $Installer141).Path; sha256 = (Get-FileHash -LiteralPath $Installer141 -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    original_version = $originalVersion
    final_version    = Get-InstalledVersion
    steps        = $records
}

# Remove the folder the current-user uninstaller leaves behind, if any, so the
# next run starts clean; the backup stays in `backup/`.
$json = Join-Path $Out 'install-matrix.json'
$result | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $json -Encoding utf8
Write-Host "Wrote $json"

$failed = @($records | Where-Object { -not $_.ok })
if ($failed.Count -gt 0) {
    Write-Host ("{0} step(s) failed" -f $failed.Count) -ForegroundColor Red
    exit 1
}
Write-Host 'Install matrix: every step passed.' -ForegroundColor Green
exit 0
