#!/usr/bin/env pwsh
# Isolated-VM acceptance runner for the checks that cannot be proven on a
# developer machine: credential binding, redirect handling, orphan authorization
# classification, and the install/upgrade/downgrade/rollback matrix.
#
# This script refuses to run unless the operator confirms a disposable VM and
# points at real release artifacts. It never invents results: every record is
# written only after the corresponding step actually ran.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('client', 'elevated')]
    [string]$Suite,
    [string]$Artifacts = $env:REMOVA_ACCEPTANCE_ARTIFACTS,
    [string]$Out = $env:REMOVA_ACCEPTANCE_OUT
)

$ErrorActionPreference = 'Stop'

# --- Guard 1: explicit disposable-VM acknowledgement ------------------------
if ($env:REMOVA_ACCEPTANCE_ISOLATED_VM -ne '1') {
    throw 'Refusing to run: set REMOVA_ACCEPTANCE_ISOLATED_VM=1 on a disposable Windows VM snapshot only.'
}

# --- Guard 2: this must look like a VM, not a physical workstation ----------
$cs = Get-CimInstance Win32_ComputerSystem
$isVm = $cs.Model -match 'Virtual|VMware|VirtualBox|KVM|Hyper-V|Parallels|QEMU'
if (-not $isVm) {
    throw "Refusing to run: '$($cs.Model)' does not look like a virtual machine. This suite deletes real services, registry keys and cache folders."
}

# --- Guard 3: the operator must have snapshotted the VM ---------------------
Write-Host ''
Write-Host 'This suite will install/uninstall software, edit PATH and services, and delete cache folders.' -ForegroundColor Yellow
Write-Host 'Confirm the VM snapshot exists and can be rolled back.' -ForegroundColor Yellow
$answer = Read-Host 'Type SNAPSHOT to continue'
if ($answer -cne 'SNAPSHOT') { throw 'Aborted: no snapshot confirmation.' }

# --- Inputs ----------------------------------------------------------------
if ([string]::IsNullOrWhiteSpace($Artifacts) -or -not (Test-Path -LiteralPath $Artifacts)) {
    throw 'Set REMOVA_ACCEPTANCE_ARTIFACTS (or -Artifacts) to the folder holding the three installers.'
}
if ([string]::IsNullOrWhiteSpace($Out)) { throw 'Set REMOVA_ACCEPTANCE_OUT (or -Out) to a results folder.' }
New-Item -ItemType Directory -Force -Path $Out | Out-Null

$isElevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if ($Suite -eq 'elevated' -and -not $isElevated) {
    throw 'The elevated suite must run from an administrator token.'
}
if ($Suite -eq 'client' -and $isElevated) {
    throw 'The client suite must run from a normal user token; run the elevated suite separately.'
}

function Write-Record {
    param(
        [string]$Id,
        [string]$Step,
        [string]$Expected,
        [string]$Actual,
        [bool]$Passed,
        [string]$Detail = ''
    )
    $record = [ordered]@{
        id         = $Id
        step       = $Step
        expected   = $Expected
        actual     = $Actual
        passed     = $Passed
        detail     = $Detail
        suite      = $Suite
        elevated   = $isElevated
        os         = (Get-CimInstance Win32_OperatingSystem).Caption
        os_version = (Get-CimInstance Win32_OperatingSystem).Version
        host_model = $cs.Model
        executed_at = (Get-Date).ToString('o')
        executor   = $env:USERNAME
    }
    $file = Join-Path $Out ("{0}-{1}.json" -f $Id, ($Step -replace '[^\w-]', '_'))
    $record | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $file -Encoding utf8
    $color = if ($Passed) { 'Green' } else { 'Red' }
    Write-Host ("[{0}] {1} :: {2}" -f $Id, $Step, $(if ($Passed) { 'PASS' } else { 'FAIL' })) -ForegroundColor $color
    if ($Detail) { Write-Host "      $Detail" -ForegroundColor DarkGray }
}

# --- Artifact identity -----------------------------------------------------
$artifactsByName = @{}
foreach ($file in Get-ChildItem -LiteralPath $Artifacts -File) {
    $artifactsByName[$file.Name] = @{
        path = $file.FullName
        sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
Write-Host ("Artifacts: {0}" -f ($artifactsByName.Keys -join ', '))

$sumsFile = Join-Path $Artifacts 'SHA256SUMS.txt'
if (Test-Path -LiteralPath $sumsFile) {
    $expectedSums = @{}
    foreach ($line in Get-Content -LiteralPath $sumsFile) {
        if ($line -match '^\s*([0-9a-fA-F]{64})\s+\*?(.+?)\s*$') {
            $expectedSums[$matches[2]] = $matches[1].ToLowerInvariant()
        }
    }
    foreach ($name in $artifactsByName.Keys) {
        $actual = $artifactsByName[$name].sha256
        $expected = $expectedSums[$name]
        $ok = $expected -and $expected -eq $actual
        Write-Record -Id 'A-05' -Step "artifact-hash:$name" `
            -Expected 'sha256 matches SHA256SUMS.txt' `
            -Actual $(if ($expected) { $actual } else { 'not listed in SHA256SUMS.txt' }) `
            -Passed ([bool]$ok)
    }
} else {
    Write-Record -Id 'A-05' -Step 'artifact-hash' -Expected 'SHA256SUMS.txt present' `
        -Actual 'missing' -Passed $false -Detail 'Without it the artifacts cannot be tied to the commit under test.'
}

# --- Suite: client ---------------------------------------------------------
if ($Suite -eq 'client') {
    # Credential, redirect and orphan-authorization checks are exercised through
    # the running app plus a loopback fixture. The steps are printed as an
    # operator checklist; the runner records what the operator observed.
    $checks = @(
        @{ Id = 'S-01'; Step = 'switch-provider-clears-key'; Expected = 'old key cleared; panel shows no saved key' },
        @{ Id = 'S-01'; Step = 'same-provider-keeps-key'; Expected = 'same provider+origin keeps the saved key' },
        @{ Id = 'S-01'; Step = 'local-endpoint-drops-cloud-key'; Expected = 'loopback endpoint receives no cloud key' },
        @{ Id = 'S-01'; Step = 'explicit-clear'; Expected = 'key removed; next request has no auth header' },
        @{ Id = 'S-02'; Step = 'redirect-301-same-origin'; Expected = 'no credential forwarded on a followed hop' },
        @{ Id = 'S-02'; Step = 'redirect-302-cross-origin'; Expected = 'target endpoint receives no key' },
        @{ Id = 'S-02'; Step = 'redirect-https-to-http'; Expected = 'downgrade refused before sending' },
        @{ Id = 'S-02'; Step = 'redirect-307-308'; Expected = 'handled without leaking the key' },
        @{ Id = 'S-03'; Step = 'shared-item-not-preselected'; Expected = 'shared candidate stays unchecked' },
        @{ Id = 'S-03'; Step = 'forged-shared-flag-refused'; Expected = 'execution refused; zero deletions' },
        @{ Id = 'S-03'; Step = 'forged-service-bound-refused'; Expected = 'execution refused; zero deletions' },
        @{ Id = 'S-03'; Step = 'unscanned-path-refused'; Expected = 'path never scanned is refused' },
        @{ Id = 'S-03'; Step = 'cancelled-scan-not-published'; Expected = 'cancelled scan authorizes nothing' },
        @{ Id = 'S-03'; Step = 'ordinary-item-still-cleans'; Expected = 'non-protected fixture cleans successfully' }
    )
    foreach ($check in $checks) {
        $observed = Read-Host ("{0} / {1} — expected: {2}. Observed PASS/FAIL/skip" -f $check.Id, $check.Step, $check.Expected)
        switch ($observed.ToLowerInvariant()) {
            'pass' { Write-Record -Id $check.Id -Step $check.Step -Expected $check.Expected -Actual 'operator observed expected behaviour' -Passed $true }
            'fail' { Write-Record -Id $check.Id -Step $check.Step -Expected $check.Expected -Actual 'operator observed a deviation' -Passed $false }
            default {
                Write-Record -Id $check.Id -Step $check.Step -Expected $check.Expected -Actual 'not executed' -Passed $false -Detail 'Skipped steps stay unproven; the report row must remain incomplete.'
            }
        }
    }
}

# --- Suite: elevated -------------------------------------------------------
if ($Suite -eq 'elevated') {
    # Install/upgrade/downgrade/rollback and the privileged credential and
    # authorization checks need an administrator token and real installers.
    $nsis = $artifactsByName.Keys | Where-Object { $_ -match 'x64-setup\.exe$' } | Select-Object -First 1
    $msi = $artifactsByName.Keys | Where-Object { $_ -match 'x64_zh-CN\.msi$' } | Select-Object -First 1
    $portable = $artifactsByName.Keys | Where-Object { $_ -match 'x64-portable\.zip$' } | Select-Object -First 1

    foreach ($pair in @(
            @{ Id = 'A-05'; Step = 'nsis-install'; Name = $nsis },
            @{ Id = 'A-05'; Step = 'msi-install'; Name = $msi },
            @{ Id = 'A-05'; Step = 'portable-extract'; Name = $portable }
        )) {
        if (-not $pair.Name) {
            Write-Record -Id $pair.Id -Step $pair.Step -Expected 'artifact present' -Actual 'artifact missing' -Passed $false
            continue
        }
        Write-Record -Id $pair.Id -Step $pair.Step -Expected "install/extract $($pair.Name) and start the app" `
            -Actual 'operator action required' -Passed $false -Detail 'Record the real outcome; this runner does not install silently.'
    }

    # Privileged DPAPI/ACL checks: the app must fail closed when the seal key is
    # unavailable, and must not silently rebuild it.
    Write-Record -Id 'S-01' -Step 'dpapi-key-unreadable-fails-closed' `
        -Expected 'app refuses AI key operations instead of writing plaintext' `
        -Actual 'operator action required' -Passed $false `
        -Detail 'Rename or ACL-block the seal key on the VM snapshot, then confirm the app reports failure.'
    Write-Record -Id 'S-03' -Step 'elevated-service-bound-item-refused' `
        -Expected 'service-bound candidate is refused under an elevated token' `
        -Actual 'operator action required' -Passed $false `
        -Detail 'Use a disposable fixture service, never a real system service.'
}

# --- Install/upgrade/rollback checklist (recorded by the operator) ----------
if ($Suite -eq 'elevated') {
    foreach ($step in @(
            'first-launch-app-list',
            'analyze-preview-clean-backup-restore',
            'migration-plan-copy-cutover-finish',
            'migration-rollback',
            'upgrade-old-to-new',
            'downgrade-new-to-old',
            'uninstall-residue-check'
        )) {
        $observed = Read-Host ("A-05 / {0} — observed PASS/FAIL/skip" -f $step)
        switch ($observed.ToLowerInvariant()) {
            'pass' { Write-Record -Id 'A-05' -Step $step -Expected 'documented in scripts/acceptance/README.md' -Actual 'operator observed expected behaviour' -Passed $true }
            'fail' { Write-Record -Id 'A-05' -Step $step -Expected 'documented in scripts/acceptance/README.md' -Actual 'operator observed a deviation' -Passed $false }
            default { Write-Record -Id 'A-05' -Step $step -Expected 'documented in scripts/acceptance/README.md' -Actual 'not executed' -Passed $false }
        }
    }
}

# --- Summary ---------------------------------------------------------------
$records = Get-ChildItem -LiteralPath $Out -Filter '*.json' | ForEach-Object {
    Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json
}
$passed = @($records | Where-Object { $_.passed }).Count
$failed = @($records | Where-Object { -not $_.passed }).Count
Write-Host ''
Write-Host ("Records: {0} passed, {1} not passed (see {2})" -f $passed, $failed, $Out)
if ($failed -gt 0) {
    Write-Host 'Not every step passed. Do not mark the corresponding report rows complete.' -ForegroundColor Yellow
    exit 1
}
Write-Host 'Every recorded step passed. Fill the report rows with these records.' -ForegroundColor Green
