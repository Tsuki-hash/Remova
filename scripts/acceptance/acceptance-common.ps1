function Confirm-AcceptanceVm {
    if ($env:REMOVA_ACCEPTANCE_ISOLATED_VM -cne '1') {
        throw 'Set REMOVA_ACCEPTANCE_ISOLATED_VM=1 on a disposable Windows VM snapshot.'
    }
    $machine = Get-CimInstance Win32_ComputerSystem
    if ($machine.Model -notmatch 'Virtual|VMware|VirtualBox|KVM|Hyper-V|Parallels|QEMU') {
        throw "The host model '$($machine.Model)' does not look like a disposable VM."
    }
    if ((Read-Host 'Confirm you have a disposable VM snapshot by typing SNAPSHOT') -cne 'SNAPSHOT') {
        throw 'Snapshot confirmation missing.'
    }
    $machine.Model
}

function Test-AcceptanceElevated {
    ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-AcceptanceWindows {
    $os = Get-CimInstance Win32_OperatingSystem
    $arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if ($arch -ne 'X64' -or $os.ProductType -ne 1 -or ([version]$os.Version).Major -ne 10) {
        throw 'The acceptance matrix requires a Windows 10/11 x64 client OS.'
    }
    [ordered]@{
        family = if ([int]$os.BuildNumber -ge 22000) { 'windows-11' } else { 'windows-10' }
        version = $os.Version
        architecture = 'x64'
    }
}

function Get-AcceptanceWebViewVersion {
    $clientId = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    $versions = @(
        foreach ($root in @(
            'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients',
            'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients'
        )) {
            Get-ItemPropertyValue -LiteralPath (Join-Path $root $clientId) -Name pv -ErrorAction SilentlyContinue
        }
    )
    $versions = @($versions | Where-Object { $_ -match '^[1-9]\d*\.\d+\.\d+\.\d+$' } | Sort-Object -Unique)
    if (-not $versions) { throw 'No installed WebView2 Runtime version was found.' }
    if (@($versions).Count -eq 1) { return $versions[0] }
    $observed = Read-Host "Installed WebView2 versions: $($versions -join ', '). Enter the version used by the candidate"
    if ($observed -notin $versions) { throw 'The observed version must match an installed WebView2 Runtime.' }
    $observed
}

function Get-AcceptanceArtifacts([string]$Directory, [string]$Version) {
    $names = [ordered]@{
        nsis = "Remova_${Version}_x64-setup.exe"
        msi = "Remova_${Version}_x64_zh-CN.msi"
        portable = "Remova_${Version}_x64-portable.zip"
    }
    $manifest = Join-Path $Directory 'SHA256SUMS.txt'
    $entries = @{}
    foreach ($line in Get-Content -LiteralPath $manifest) {
        if ($line -match '^\s*([0-9a-fA-F]{64})\s+\*?(.+?)\s*$') {
            $digest, $name = $Matches[1], $Matches[2]
            if ($entries.ContainsKey($name)) { throw "Duplicate checksum entry for $name." }
            $entries[$name] = $digest.ToLowerInvariant()
        }
    }
    $artifacts = [ordered]@{}
    $checks = foreach ($channel in $names.Keys) {
        $name = $names[$channel]
        $path = Join-Path $Directory $name
        $sha = if (Test-Path -LiteralPath $path -PathType Leaf) {
            (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        }
        $artifacts[$channel] = [ordered]@{ name = $name; sha256 = $sha }
        $ok = $sha -and $entries[$name] -ceq $sha
        [ordered]@{
            id = "artifact-$channel"
            expected = "$name matches its SHA256SUMS.txt entry"
            actual = if ($sha) { "SHA-256 $sha; manifest $($entries[$name])" } else { 'Missing installer' }
            status = if ($ok) { 'pass' } else { 'fail' }
        }
    }
    [pscustomobject]@{ artifacts = $artifacts; checks = @($checks) }
}

function Read-AcceptanceExecutableReference([string]$Path, $Artifacts) {
    $reference = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json -AsHashtable
    $channels = @('nsis', 'msi', 'portable')
    if ($reference -isnot [Collections.IDictionary] -or $reference.Count -ne $channels.Count -or
        (Compare-Object @($reference.Keys) $channels -CaseSensitive)) {
        throw 'The executable reference must identify all three package channels.'
    }
    $digests = [ordered]@{}
    foreach ($channel in $channels) {
        $entry = $reference[$channel]
        if ($entry -isnot [Collections.IDictionary] -or $entry.Count -ne 2 -or
            $entry.package_sha256 -isnot [string] -or $entry.package_sha256 -cne $Artifacts[$channel].sha256 -or
            $entry.executable_sha256 -isnot [string] -or $entry.executable_sha256 -cnotmatch '^[a-f0-9]{64}$' -or
            $entry.executable_sha256 -cmatch '^0+$') {
            throw "Invalid executable reference or different $channel package."
        }
        $digests[$channel] = $entry.executable_sha256
    }
    $digests
}

function Get-PortableExeHash([string]$Path, [string]$ArchiveHash) {
    $file = [IO.File]::Open((Resolve-Path -LiteralPath $Path).Path, 'Open', 'Read', 'Read')
    try {
        if ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($file)).ToLowerInvariant() -cne $ArchiveHash) {
            throw 'The portable attachment changed during verification.'
        }
        $file.Position = 0
        $archive = [IO.Compression.ZipArchive]::new($file, 'Read', $true)
        try {
            $entries = @($archive.Entries | Where-Object { $_.FullName -ieq 'remova.exe' })
            if ($entries.Count -ne 1 -or $entries[0].Length -le 0) {
                throw 'The portable archive must contain one nonempty remova.exe at its root.'
            }
            $exe = $entries[0].Open()
            try { [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($exe)).ToLowerInvariant() }
            finally { $exe.Dispose() }
        } finally { $archive.Dispose() }
    } finally { $file.Dispose() }
}

function Test-AcceptanceExecutableVersion([string]$Observed, [string]$Expected) {
    $Observed -cin @($Expected, "$Expected.0")
}

function Test-AcceptanceExecutablePath([string]$Path) {
    $Path -match '^[a-z]:[\\/](?:.*[\\/])?remova\.exe$' -and $Path -notmatch '[\\/]\.\.?[\\/]'
}

function Get-AcceptanceLaunchObservation([string]$Path, [string]$Version, [string]$ExpectedSha256) {
    if (-not (Test-AcceptanceExecutablePath $Path)) {
        throw 'Enter the full path of the installed or extracted remova.exe.'
    }
    if (Get-Process -Name Remova -ErrorAction SilentlyContinue) {
        throw 'Close existing Remova processes before observing this executable.'
    }
    $path = (Get-Item -LiteralPath $Path).FullName
    $pin = [IO.File]::Open($path, 'Open', 'Read', 'Read')
    try {
        $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($pin)).ToLowerInvariant()
        $productVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($path).ProductVersion
        if ($hash -cne $ExpectedSha256 -or -not (Test-AcceptanceExecutableVersion $productVersion $Version)) {
            throw 'The executable does not match the reviewed package reference or candidate version.'
        }
        $process = Start-Process -FilePath $path -PassThru
        try {
            if (-not (Wait-RemovaUiReady $process)) { throw 'The process did not show the enabled Software navigation button.' }
            $actualPath = $process.MainModule.FileName
            if ($process.HasExited -or $actualPath -ine $path) { throw 'The running process did not match the selected executable.' }
            [ordered]@{
                path = $actualPath
                version = $productVersion
                sha256 = $hash
                pid = $process.Id
                uia_ready = $true
            }
        } finally { Close-RemovaTestProcess $process }
    } finally { $pin.Dispose() }
}

function New-AcceptanceObservation($Check, [string]$Status, [string]$Actual, [string]$EvidencePath) {
    $status = $Status.Trim().ToLowerInvariant()
    $record = [ordered]@{
        id = $Check.id
        expected = $Check.expected
        actual = $Actual.Trim()
        status = if ($status -in @('pass', 'fail')) { $status } else { 'skip' }
    }
    if ($record.status -eq 'skip') {
        if (-not $record.actual) { $record.actual = 'Not executed' }
        return $record
    }
    if ($EvidencePath -and (Test-Path -LiteralPath $EvidencePath -PathType Leaf)) {
        $file = Get-Item -LiteralPath $EvidencePath
        if ($file.Length -gt 0) {
            $record.evidence = [ordered]@{
                name = $file.Name
                sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            }
        }
    }
    if ($record.status -eq 'pass' -and (-not $record.actual -or -not $record.evidence)) {
        $record.status = 'fail'
        $record.actual = "Incomplete evidence (an observation and nonempty evidence file are required). $($record.actual)".Trim()
    }
    $record
}

# Input idle alone can be observed on a blank or failed WebView. A visible,
# enabled React navigation button establishes that the application UI rendered.
function Wait-RemovaUiReady([Diagnostics.Process]$Process, [int]$TimeoutMs = 30000) {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $element = [System.Windows.Automation.AutomationElement]
    $button = [System.Windows.Automation.PropertyCondition]::new(
        $element::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
    $enabled = [System.Windows.Automation.PropertyCondition]::new($element::IsEnabledProperty, $true)
    $visible = [System.Windows.Automation.PropertyCondition]::new($element::IsOffscreenProperty, $false)
    $english = [System.Windows.Automation.PropertyCondition]::new($element::NameProperty, 'Software')
    $chinese = [System.Windows.Automation.PropertyCondition]::new($element::NameProperty, '软件')
    $name = [System.Windows.Automation.OrCondition]::new($english, $chinese)
    $filter = [System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.Condition[]]@($button, $enabled, $visible, $name))
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($timer.ElapsedMilliseconds -lt $TimeoutMs) {
        $Process.Refresh()
        if ($Process.HasExited) { return $false }
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero -and $Process.WaitForInputIdle(100)) {
            try {
                $root = $element::FromHandle($Process.MainWindowHandle)
                if ($root -and $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $filter)) {
                    return $timer.ElapsedMilliseconds -lt $TimeoutMs -and -not $Process.HasExited
                }
            } catch [System.Windows.Automation.ElementNotAvailableException] {
                # The WebView can replace its accessible tree during startup.
            }
        }
        Start-Sleep -Milliseconds 25
    }
    $false
}

function Close-RemovaTestProcess([Diagnostics.Process]$Process) {
    try {
        if (-not $Process.HasExited -and (-not $Process.CloseMainWindow() -or -not $Process.WaitForExit(2000))) {
            $Process.Kill()
            if (-not $Process.WaitForExit(10000)) { throw 'The test process did not exit.' }
        }
    } finally {
        $Process.Dispose()
    }
}

function Get-AcceptanceChecks([ValidateSet('client', 'elevated')][string]$Suite) {
    foreach ($channel in @('nsis', 'msi', 'portable')) {
        @{ id = "artifact-$channel"; expected = 'Package matches the release checksums'; requires_evidence = $false }
    }
    $manual = if ($Suite -eq 'client') {
        @(
            @{ id = 'switch-provider-clears-key'; expected = 'Changing providers clears the saved key' },
            @{ id = 'same-provider-keeps-key'; expected = 'Same provider and origin keep the saved key' },
            @{ id = 'local-endpoint-drops-cloud-key'; expected = 'Loopback endpoint receives no cloud key' },
            @{ id = 'explicit-clear'; expected = 'Clearing the key removes the authentication header' },
            @{ id = 'redirect-301-same-origin'; expected = 'The authenticated request does not follow the redirect' },
            @{ id = 'redirect-302-cross-origin'; expected = 'The other endpoint receives no credential' },
            @{ id = 'redirect-https-to-http'; expected = 'The downgrade is refused before sending' },
            @{ id = 'redirect-307-308'; expected = 'The authenticated request does not follow either redirect' },
            @{ id = 'shared-item-not-preselected'; expected = 'A shared candidate remains unchecked' },
            @{ id = 'forged-shared-flag-refused'; expected = 'Execution refuses the tampered candidate and deletes nothing' },
            @{ id = 'forged-service-bound-refused'; expected = 'Execution refuses the tampered service-bound candidate' },
            @{ id = 'unscanned-path-refused'; expected = 'A path outside the latest scan is refused' },
            @{ id = 'cancelled-scan-not-published'; expected = 'A cancelled scan does not authorize cleanup' },
            @{ id = 'ordinary-item-still-cleans'; expected = 'A non-protected fixture cleans successfully' }
        )
    } else {
        @(
            @{ id = 'ai-dpapi-unavailable'; expected = 'A failed AI credential DPAPI operation never writes a plaintext key' },
            @{ id = 'backup-seal-key-unavailable'; expected = 'A missing or unreadable backup seal key blocks automatic restore' },
            @{ id = 'elevated-service-bound-item-refused'; expected = 'A disposable service-bound candidate is refused under an administrator token' }
        )
    }

    $channelChecks = foreach ($channel in @('nsis', 'msi', 'portable')) {
        $startup = switch ($channel) {
            'nsis' { 'Install the per-user package, start the installed app and load the inventory' }
            'msi' {
                if ($Suite -eq 'elevated') { 'Install the per-machine package, start the installed app and load the inventory' }
                else { 'Start the preinstalled per-machine package as a standard user and load the inventory' }
            }
            'portable' { 'Extract the portable package, start the extracted app and load the inventory' }
        }
        @{ id = "$channel-first-launch"; expected = $startup }
        @{ id = "$channel-cleanup-restore"; expected = 'Analyze, preview, clean, back up and restore an isolated fixture with the expected permission behavior' }
        @{ id = "$channel-migration-finish"; expected = 'Plan, copy, cut over and finish migration on an isolated fixture without data loss' }
        @{ id = "$channel-migration-rollback"; expected = 'Roll back a second fixture and verify pre- and post-cutover data' }
        @{ id = "$channel-upgrade-downgrade"; expected = 'Upgrade from the stable release and downgrade using the same channel; verify the configuration and data' }
        @{ id = "$channel-uninstall"; expected = 'Uninstall or remove the portable package and check the expected leftovers' }
    }
    $manual = @($manual) + @($channelChecks)
    foreach ($check in $manual) {
        $check.requires_evidence = $true
        $check
    }
}
