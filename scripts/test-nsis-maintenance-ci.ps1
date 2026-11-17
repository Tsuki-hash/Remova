# Run both real NSIS maintenance branches on the Windows packaging runner.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$fixture = Join-Path $PSScriptRoot 'test-nsis-maintenance.ps1'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$source = (Resolve-Path (Join-Path $repo 'src-tauri/target/release/nsis/x64')).Path
$toolDir = (Resolve-Path (Join-Path $env:LOCALAPPDATA 'tauri/NSIS')).Path
& $fixture -ExpectedToken Elevated -TemplateDirectory $source

# A separate non-admin account works even on runners without a split UAC
# token. It gets a private registry hive and runs only the staged fixture.
$parent = (Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path
$nonce = [guid]::NewGuid().ToString('N')
$stage = Join-Path $parent "remova-nsis-token-$nonce"
$userName = 'RemovaTest' + $nonce.Substring(0, 8)
$password = ConvertTo-SecureString (([guid]::NewGuid().ToString('N')) + '!aB7') -AsPlainText -Force
$account = $null
try {
    $null = New-Item -ItemType Directory -Path $stage
    $account = New-LocalUser -Name $userName -Password $password -Description 'Temporary NSIS test account'
    Add-LocalGroupMember -SID ([Security.Principal.SecurityIdentifier]::new('S-1-5-32-545')) -Member $account
    $output = (New-Item -ItemType Directory -Path (Join-Path $stage 'output')).FullName
    $build = (New-Item -ItemType Directory -Path (Join-Path $stage 'src-tauri/target/release') -Force).FullName
    $template = (New-Item -ItemType Directory -Path (Join-Path $build 'nsis/x64') -Force).FullName
    foreach ($file in 'installer.nsi', 'utils.nsh', 'FileAssociation.nsh', 'English.nsh', 'SimpChinese.nsh') {
        Copy-Item -LiteralPath (Join-Path $source $file) -Destination $template
    }
    foreach ($binary in 'list_apps.exe', 'verify_update.exe') {
        Copy-Item -LiteralPath (Join-Path $repo "src-tauri/target/release/$binary") -Destination $build
    }
    Copy-Item -LiteralPath $toolDir -Destination (Join-Path $stage 'NSIS') -Recurse
    Copy-Item -LiteralPath $fixture, (Join-Path $repo 'LICENSE'), (Join-Path $repo 'THIRD-PARTY-NOTICES.md') -Destination $stage
    $static = Join-Path $stage 'src-tauri'
    Copy-Item -LiteralPath (Join-Path $repo 'src-tauri/windows'), (Join-Path $repo 'src-tauri/icons') -Destination $static -Recurse
    $rendered = Join-Path $template 'installer.nsi'
    $content = (Get-Content -Raw -LiteralPath $rendered).Replace($repo, $stage, [StringComparison]::OrdinalIgnoreCase)
    $content.Replace($toolDir, (Join-Path $stage 'NSIS'), [StringComparison]::OrdinalIgnoreCase) |
        Set-Content -LiteralPath $rendered -Encoding utf8BOM
    & icacls.exe $stage /grant "*$($account.SID.Value):(OI)(CI)(RX)" | Out-Null
    if ($LASTEXITCODE) { throw 'Cannot grant the NSIS test account access to the fixture.' }
    & icacls.exe $output /grant "*$($account.SID.Value):(OI)(CI)(M)" | Out-Null
    if ($LASTEXITCODE) { throw 'Cannot grant the NSIS test account access to the evidence folder.' }

    $worker = @'
param([string]$Root)
$ErrorActionPreference = 'Stop'
$env:TEMP = $env:TMP = Join-Path $Root 'output'
try {
    & "$Root/test-nsis-maintenance.ps1" -ExpectedToken Standard -TemplateDirectory "$Root/src-tauri/target/release/nsis/x64" -MakeNsis "$Root/NSIS/makensis.exe" *> "$Root/output/standard.log"
    exit 0
} catch {
    $_ | Out-String | Add-Content -LiteralPath "$Root/output/standard.log"
    exit 1
}
'@
    $worker | Set-Content -LiteralPath (Join-Path $stage 'run-standard.ps1') -Encoding utf8
    $credential = [pscredential]::new("$env:COMPUTERNAME\$userName", $password)
    $process = Start-Process -FilePath (Join-Path $PSHOME 'pwsh.exe') -Credential $credential -LoadUserProfile `
        -WorkingDirectory $stage -WindowStyle Hidden -PassThru `
        -ArgumentList '-NoProfile', '-NonInteractive', '-File', "`"$stage/run-standard.ps1`"", "`"$stage`""
    try {
        if (!$process.WaitForExit(180000)) {
            $process.Kill($true)
            $process.WaitForExit()
            throw 'The standard-token NSIS fixture timed out.'
        }
        $log = Get-Content -Raw -LiteralPath (Join-Path $output 'standard.log')
        Write-Host $log.Trim()
        if ($process.ExitCode -ne 0 -or $log -notmatch 'fixture passed: standard setup' -or
            $log -notmatch 'fixture passed: invalid paths and commands refused') {
            throw 'The standard-token NSIS fixture did not complete both checks.'
        }
    } finally { $process.Dispose() }
} finally {
    if ($account) { Remove-LocalUser -SID $account.SID }
    $actual = Get-Item -LiteralPath $stage -ErrorAction SilentlyContinue
    if ($actual -and $actual.Parent.FullName -eq $parent -and $actual.Name -ceq "remova-nsis-token-$nonce" -and
        -not ($actual.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        Remove-Item -LiteralPath $actual.FullName -Recurse -Force
    }
}
