#!/usr/bin/env pwsh
# Test the per-user NSIS install, upgrade, downgrade and uninstall in a
# disposable Windows VM. Restore the original environment by reverting the VM
# snapshot, including when a step fails.
param(
    [Parameter(Mandatory = $true)][string]$Installer140,
    [Parameter(Mandatory = $true)][string]$Installer141,
    [Parameter(Mandatory = $true)][string]$InstallDir,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-fA-F]{64}$')][string]$CandidateExeSha256,
    [string]$ConfigDir = '',
    [string[]]$FixtureFiles = @(),
    [string]$Out = '',
    [switch]$ConfirmReplaceInstalledApp
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance-common.ps1"

function Add-MatrixResult([string]$Step, [bool]$Ok, [string]$Detail) {
    $records.Add([ordered]@{ step = $Step; ok = $Ok; detail = $Detail; at = (Get-Date).ToString('o') })
    $label = if ($Ok) { 'ok' } else { 'FAIL' }
    Write-Host "[$label] ${Step}: $Detail"
}

function Assert-MatrixResult([string]$Step, [bool]$Condition, [string]$Detail) {
    Add-MatrixResult $Step $Condition $Detail
    if (-not $Condition) { throw "${Step} failed: $Detail" }
}

function Invoke-NsisInstall([string]$Installer, [string]$Directory) {
    # NSIS: /D= must be the last argument and unquoted, including paths with spaces.
    (Start-Process -FilePath $Installer -ArgumentList @('/S', "/D=$Directory") -PassThru -Wait).ExitCode
}

function Test-MatrixAppLaunch([string]$Exe) {
    $process = Start-Process -FilePath $Exe -PassThru
    try {
        Wait-RemovaUiReady $process
    } finally {
        Close-RemovaTestProcess $process
    }
}

function Assert-MatrixInstall([string]$Step, [string]$Directory, [string]$Version, [string]$ExpectedExeSha = '') {
    $entry = Get-ItemProperty -LiteralPath $uninstallKey -ErrorAction SilentlyContinue
    $location = [string]$entry.InstallLocation
    if ($location -match '^"([^"]+)"$') { $location = $Matches[1] }
    $actualLocation = if ($location -and $location -notmatch '"' -and [IO.Path]::IsPathFullyQualified($location)) {
        [IO.Path]::TrimEndingDirectorySeparator([IO.Path]::GetFullPath($location))
    }
    Assert-MatrixResult "$Step-location" ($actualLocation -ieq $Directory) "registry=$actualLocation; expected=$Directory"

    $exe = Join-Path $Directory 'remova.exe'
    $file = Get-Item -LiteralPath $exe -ErrorAction SilentlyContinue
    $actualVersion = $file.VersionInfo.ProductVersion
    $versionMatches = $entry.DisplayVersion -eq $Version -and $actualVersion -in @($Version, "$Version.0")
    Assert-MatrixResult "$Step-version" $versionMatches "registry=$($entry.DisplayVersion); exe=$actualVersion; expected=$Version"

    if ($ExpectedExeSha) {
        $actualSha = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
        Assert-MatrixResult "$Step-exe-sha256" ($actualSha -ieq $ExpectedExeSha) "exe=$actualSha; expected=$ExpectedExeSha"
    }
    Assert-MatrixResult "$Step-launches" (Test-MatrixAppLaunch $exe) 'The installed application exposes its Software navigation button.'
}

function Assert-MatrixFixtures([string]$Step, $Baseline) {
    foreach ($path in $Baseline.Keys) {
        $digest = if (Test-Path -LiteralPath $path -PathType Leaf) {
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
        }
        Assert-MatrixResult $Step ($digest -eq $Baseline[$path]) "Fixture file remains byte-identical: $path"
    }
}

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Out) {
    $runName = 'install-matrix-{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 8))
    $Out = Join-Path $repoRoot "benchmarks/runs/$runName"
}
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path -LiteralPath $Out).Path
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Remova'
$records = [System.Collections.Generic.List[object]]::new()
$result = [ordered]@{
    kind = 'install-matrix'
    install_dir = $InstallDir
    expected_exe_sha256 = $CandidateExeSha256.ToLowerInvariant()
    snapshot_rollback_required = $false
    steps = $records
    passed = $false
}

try {
    if (-not $ConfirmReplaceInstalledApp) {
        throw 'This matrix replaces the installed app in a disposable VM. Pass -ConfirmReplaceInstalledApp.'
    }
    $result.host_model = Confirm-AcceptanceVm
    $result.elevated = Test-AcceptanceElevated
    if ($result.elevated) { throw 'Run the per-user NSIS matrix from the standard-user token.' }
    $windows = Get-AcceptanceWindows
    $result.os_family = $windows.family
    $result.os_version = $windows.version
    $result.architecture = $windows.architecture
    $result.webview2_version = Get-AcceptanceWebViewVersion

    if (Get-Process -Name Remova -ErrorAction SilentlyContinue) {
        throw 'Close the Remova process before running the matrix.'
    }
    if (-not [IO.Path]::IsPathFullyQualified($InstallDir)) { throw 'Use an absolute installation directory.' }
    $InstallDir = [IO.Path]::TrimEndingDirectorySeparator([IO.Path]::GetFullPath($InstallDir))
    if ($InstallDir -eq [IO.Path]::GetPathRoot($InstallDir)) { throw 'The installation directory cannot be a drive root.' }
    $defaultDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Remova'
    foreach ($appDir in @($InstallDir, $defaultDir)) {
        if ($Out -ieq $appDir -or $Out.StartsWith("$appDir\", [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Keep matrix evidence outside the directories the installer may replace.'
        }
    }
    $result.install_dir = $InstallDir
    $result.installers = [ordered]@{}
    foreach ($item in @(@{ id = 'v140'; path = $Installer140 }, @{ id = 'v141'; path = $Installer141 })) {
        $path = (Get-Item -LiteralPath $item.path).FullName
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Installer not found: $path" }
        $result.installers[$item.id] = @{
            path = $path
            sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }

    if (-not $ConfigDir) {
        $ConfigDir = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Remova'
    }
    if (-not $FixtureFiles.Count) {
        $historyDir = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'Remova'
        $FixtureFiles = @((Join-Path $ConfigDir 'ai-config.json'), (Join-Path $historyDir 'history.jsonl'))
    }
    $fixtureHashes = [ordered]@{}
    foreach ($fixture in $FixtureFiles) {
        $item = Get-Item -LiteralPath $fixture -ErrorAction SilentlyContinue
        if (-not $item -or $item.PSIsContainer -or -not $item.Length) {
            throw "Evidence insufficient: seed a nonempty Remova fixture before the matrix: $fixture"
        }
        $fixtureHashes[$item.FullName] = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash
    }
    if ($fixtureHashes.Count -lt 2) { throw 'Seed separate Remova configuration and data fixtures.' }
    $result.fixture_files = $fixtureHashes
    Add-MatrixResult 'preflight' $true "Isolated VM snapshot confirmed. Fixture files: $($fixtureHashes.Count)."

    $result.snapshot_rollback_required = $true
    $code = Invoke-NsisInstall $result.installers.v140.path $InstallDir
    Assert-MatrixResult 'install-1.4.0' ($code -eq 0) "exit=$code"
    Assert-MatrixInstall 'install-1.4.0' $InstallDir '1.4.0'
    Assert-MatrixFixtures 'install-keeps-fixture' $fixtureHashes

    $code = Invoke-NsisInstall $result.installers.v141.path $InstallDir
    Assert-MatrixResult 'upgrade-1.4.1' ($code -eq 0) "exit=$code"
    Assert-MatrixInstall 'upgrade-1.4.1' $InstallDir '1.4.1' $CandidateExeSha256
    $entries = @(Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' |
        ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -like '*Remova*' })
    Assert-MatrixResult 'upgrade-single-entry' ($entries.Count -eq 1) "uninstall entries=$($entries.Count)"
    Assert-MatrixFixtures 'upgrade-keeps-fixture' $fixtureHashes

    $code = Invoke-NsisInstall $result.installers.v140.path $InstallDir
    Assert-MatrixResult 'rollback-1.4.0' ($code -eq 0) "exit=$code"
    Assert-MatrixInstall 'rollback-1.4.0' $InstallDir '1.4.0'
    Assert-MatrixFixtures 'rollback-keeps-fixture' $fixtureHashes

    $code = Invoke-NsisInstall $result.installers.v141.path $InstallDir
    Assert-MatrixResult 'reinstall-1.4.1' ($code -eq 0) "exit=$code"
    Assert-MatrixInstall 'reinstall-1.4.1' $InstallDir '1.4.1' $CandidateExeSha256
    Assert-MatrixFixtures 'reinstall-keeps-fixture' $fixtureHashes

    $uninstaller = Join-Path $InstallDir 'uninstall.exe'
    $code = (Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru -Wait).ExitCode
    Assert-MatrixResult 'uninstall-1.4.1' ($code -eq 0) "exit=$code"
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ((Test-Path -LiteralPath $uninstallKey) -and $timer.Elapsed.TotalSeconds -lt 60) {
        Start-Sleep -Milliseconds 250
    }
    Assert-MatrixResult 'uninstall-key-removed' (-not (Test-Path -LiteralPath $uninstallKey)) 'Uninstall registry entry absent.'
    $leftovers = @(Get-ChildItem -LiteralPath $InstallDir -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
    $binaries = @($leftovers | Where-Object { $_ -in @('remova.exe', 'list_apps.exe', 'uninstall.exe', 'verify_update.exe') })
    Assert-MatrixResult 'uninstall-binaries-removed' ($binaries.Count -eq 0) "installed binaries left=$($binaries -join ',')"
    Add-MatrixResult 'uninstall-leftovers' $true "remaining runtime files=$($leftovers -join ',')"
    $result.passed = $true
} catch {
    $result.error = $_.Exception.Message
    Add-MatrixResult 'matrix-stopped' $false $result.error
} finally {
    $result.generated_at = (Get-Date).ToUniversalTime().ToString('o')
    $json = Join-Path $Out 'install-matrix.json'
    $result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $json -Encoding utf8
    Write-Host "Wrote $json"
    if ($result.snapshot_rollback_required) {
        Write-Host 'Export the evidence and revert the VM snapshot to restore the pre-test app, registry and configuration.' -ForegroundColor Yellow
    }
}
if (-not $result.passed) { exit 1 }
