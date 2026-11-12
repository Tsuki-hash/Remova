# Exercise the rendered installer with isolated product and registry names.
[CmdletBinding()]
param(
    [string]$MakeNsis = (Join-Path $env:LOCALAPPDATA 'tauri/NSIS/makensis.exe'),
    [switch]$VerifyElevation
)

$ErrorActionPreference = 'Stop'
$source = Join-Path $PSScriptRoot '../src-tauri/target/release/nsis/x64'
$template = Get-Content -Raw -LiteralPath (Join-Path $source 'installer.nsi')
$MakeNsis = (Resolve-Path -LiteralPath $MakeNsis).Path
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$elevated = [Security.Principal.WindowsPrincipal]::new($identity).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)

function Invoke-Setup([string]$Exe, [string]$Arguments, [int]$Expected = 0) {
    $process = Start-Process -FilePath $Exe -ArgumentList $Arguments -Wait -PassThru -WindowStyle Hidden
    try {
        if ($process.ExitCode -ne $Expected) {
            throw "$([IO.Path]::GetFileName($Exe)) $Arguments exited $($process.ExitCode), expected $Expected."
        }
    } finally { $process.Dispose() }
}

$parent = (Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path
$product = 'RemovaSetupFixture-' + [guid]::NewGuid().ToString('N')
$lab = (New-Item -ItemType Directory -Path (Join-Path $parent $product)).FullName
$payload = Join-Path $lab 'remova.exe'
$install = Join-Path $lab 'custom install'
$uninstaller = Join-Path $install 'uninstall.exe'
$binary = Join-Path $install 'remova.exe'
$sentinel = Join-Path $lab 'keep.txt'
$marker = Join-Path $lab 'unexpected-child.txt'
$alias = Join-Path $lab 'directory alias'
$vendorKey = "HKCU:\Software\$product"
$appKey = Join-Path $vendorKey $product
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\$product"

function Build-Setup([string]$FileName, [string]$Version, [switch]$PrepareUninstaller) {
    $exe = Join-Path $lab $FileName
    $defines = @{
        PRODUCTNAME = $product; MANUFACTURER = $product; BUNDLEID = $product; STARTMENUFOLDER = $product
        OUTFILE = $exe; VERSION = $Version; VERSIONWITHBUILD = "$Version.0"
        MAINBINARYSRCPATH = $payload
        INSTALLWEBVIEW2MODE = 'skip'; DISPLAYLANGUAGESELECTOR = 'false'
    }
    $script = $template
    foreach ($name in $defines.Keys) {
        $pattern = [regex]::new('(?m)^!define ' + $name + ' "[^"\r\n]*"(?=\r?$)')
        if ($pattern.Matches($script).Count -ne 1) { throw "Missing or repeated NSIS define: $name" }
        $script = $pattern.Replace($script, [System.Text.RegularExpressions.MatchEvaluator]{
            param($match)
            '!define ' + $name + ' "' + $defines[$name] + '"'
        })
    }
    if ($script -notmatch '(?m)^!define INSTALLMODE "currentUser"\r?$' -or
        $script -notmatch '(?m)^!define MAINBINARYNAME "remova"\r?$' -or
        $script -match '(?im)^\s*(?:ReadRegStr|ReadRegExpandStr)\s+[^\r\n]+"MainBinaryName"') {
        throw 'Unexpected per-user NSIS package configuration.'
    }

    $init = [regex]::new('(?ms)^Function \.onInit\r?\n(.*?)^FunctionEnd')
    $original = $init.Match($script).Groups[1].Value
    $guard = '    !insertmacro RemovaRequireCurrentUserToken'
    if (!$original.Replace("`r", '').StartsWith(('  !if "${INSTALLMODE}" == "currentUser"' + "`n" + $guard))) {
        throw 'Expected the installer to check elevation before its other initialization.'
    }
    if ($PrepareUninstaller) {
        # Seed an uninstaller on elevated CI hosts. Its own initialization is unmodified.
        $updated = $original.Replace($guard, '')
    } else {
        # Exercise the real command builder without automating the interactive page.
        $probe = @'
  ${GetOptions} $CMDLINE "/MAINTPROBE" $0
  ${IfNot} ${Errors}
    !insertmacro RemovaPrepareUserUninstall
    ExecWait '$R1 /S _?=$4' $0
    IfErrors 0 +2
      StrCpy $0 2
    SetErrorLevel $0
    Quit
  ${EndIf}
'@
        $updated = $original.Replace('  !insertmacro SetContext', "  !insertmacro SetContext`n$probe")
        if ($updated -ceq $original) { throw 'Cannot insert the maintenance fixture.' }
    }
    $script = $init.Replace($script, [System.Text.RegularExpressions.MatchEvaluator]{
        param($match)
        "Function .onInit`n${updated}FunctionEnd"
    }, 1)
    $scriptPath = Join-Path $lab "$FileName.nsi"
    $script | Set-Content -LiteralPath $scriptPath -Encoding utf8BOM
    & $MakeNsis /V2 $scriptPath | Out-Host
    if ($LASTEXITCODE) { throw "Could not compile $FileName." }
    return $exe
}

function Assert-Installed([string]$Version) {
    if ((Get-ItemProperty -LiteralPath $uninstallKey).DisplayVersion -cne $Version -or
        !(Test-Path -LiteralPath $binary -PathType Leaf) -or
        !(Test-Path -LiteralPath $uninstaller -PathType Leaf) -or
        (Get-Content -Raw -LiteralPath $sentinel) -cne 'keep') {
        throw "Expected version $Version at the original directory, with the unrelated file intact."
    }
}

try {
    foreach ($include in 'utils.nsh', 'FileAssociation.nsh', 'English.nsh', 'SimpChinese.nsh') {
        Copy-Item -LiteralPath (Join-Path $source $include) -Destination $lab
    }
    [IO.File]::WriteAllText($payload, 'NSIS maintenance fixture')
    [IO.File]::WriteAllText($sentinel, 'keep')
    $current = Build-Setup 'current.exe' '1.4.1'
    if ($elevated) {
        Invoke-Setup $current "/S /NS /D=$install" 5
        if ((Test-Path -LiteralPath $install) -or (Test-Path -LiteralPath $vendorKey) -or
            (Test-Path -LiteralPath $uninstallKey)) {
            throw 'Elevated setup wrote installation state.'
        }
        $seed = Build-Setup 'seed.exe' '1.4.0' -PrepareUninstaller
        Invoke-Setup $seed "/S /NS /D=$install"
    } else {
        $prior = Build-Setup 'prior.exe' '1.4.0'
        Invoke-Setup $prior "/S /NS /D=$install"
    }
    Assert-Installed '1.4.0'

    $validCommand = '"{0}"' -f $uninstaller
    if ($elevated) {
        Set-ItemProperty -LiteralPath $uninstallKey -Name 'UninstallString' -Value "$validCommand /unexpected"
        Set-ItemProperty -LiteralPath $uninstallKey -Name 'MainBinaryName' -Value '..\keep.txt'
        Invoke-Setup $current '/S /NS' 5
        Invoke-Setup $current '/S /MAINTPROBE' 5
        Invoke-Setup $uninstaller "/S _?=$install" 5
        Assert-Installed '1.4.0'
        if ((Get-ItemProperty -LiteralPath $uninstallKey).UninstallString -cne "$validCommand /unexpected") {
            throw 'Elevated maintenance changed existing installation state.'
        }
        Write-Host 'NSIS fixture passed: elevated fresh setup, maintenance and uninstall refused'
    } else {
        $child = @'
Unicode true
Name "Alternate uninstaller fixture"
OutFile "${OUTPUT}"
RequestExecutionLevel user
SilentInstall silent
Section
  FileOpen $0 "${MARKER}" w
  FileWrite $0 "unexpected execution"
  FileClose $0
SectionEnd
'@
        $alternate = Join-Path $lab 'alternate.exe'
        $childScript = Join-Path $lab 'alternate.nsi'
        $child | Set-Content -LiteralPath $childScript -Encoding utf8
        & $MakeNsis /V2 "/DOUTPUT=$alternate" "/DMARKER=$marker" $childScript | Out-Host
        if ($LASTEXITCODE) { throw 'Could not compile alternate uninstaller.' }

        New-Item -ItemType Junction -Path $alias -Target $install | Out-Null
        $invalid = @(
            @{ Directory = $install; Command = ('"{0}"' -f $alternate) },
            @{ Directory = $install; Command = "$validCommand /unexpected" },
            @{ Directory = $alias; Command = ('"{0}\uninstall.exe"' -f $alias) },
            @{ Directory = "$install\..\custom install"; Command = ('"{0}\..\custom install\uninstall.exe"' -f $install) }
        )
        foreach ($case in $invalid) {
            Set-Item -LiteralPath $appKey -Value $case.Directory
            Set-ItemProperty -LiteralPath $uninstallKey -Name 'UninstallString' -Value $case.Command
            Invoke-Setup $current '/S /MAINTPROBE' 5
            Assert-Installed '1.4.0'
        }
        if (Test-Path -LiteralPath $marker) { throw 'An alternate command was executed.' }
        Set-Item -LiteralPath $appKey -Value $install
        Set-ItemProperty -LiteralPath $uninstallKey -Name 'UninstallString' -Value $validCommand
        Set-ItemProperty -LiteralPath $uninstallKey -Name 'MainBinaryName' -Value '..\keep.txt'
        Invoke-Setup $current '/S /NS /UPDATE'
        Assert-Installed '1.4.1'
        Invoke-Setup $current '/S /NS'
        Assert-Installed '1.4.1'
        Invoke-Setup $current '/S /MAINTPROBE'
        if ((Test-Path -LiteralPath $binary) -or (Test-Path -LiteralPath $uninstallKey)) {
            throw 'Validated maintenance did not uninstall the fixture.'
        }
        Invoke-Setup $current "/S /NS /D=$install"
        Invoke-Setup $uninstaller '/S'
        if ((Test-Path -LiteralPath $binary) -or (Test-Path -LiteralPath $uninstallKey) -or
            (Get-Content -Raw -LiteralPath $sentinel) -cne 'keep') {
            throw 'Uninstall did not preserve the unrelated file or left the fixture installed.'
        }
        Write-Host 'NSIS fixture passed: standard setup, upgrade, reinstall, validated maintenance and uninstall'
        Write-Host 'NSIS fixture passed: invalid paths and commands refused, unrelated file preserved'
    }

    if ($VerifyElevation -and !$elevated) {
        $log = Join-Path $lab 'elevated.log'
        $command = "& '$($PSCommandPath.Replace("'", "''"))' -MakeNsis '$($MakeNsis.Replace("'", "''"))' *> '$($log.Replace("'", "''"))'"
        $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
        $process = Start-Process -FilePath (Get-Process -Id $PID).Path -Verb RunAs -WindowStyle Hidden -PassThru -Wait -ArgumentList '-NoProfile', '-EncodedCommand', $encoded
        try {
            $result = Get-Content -LiteralPath $log -Raw
            Write-Host $result.Trim()
            if ($process.ExitCode -ne 0 -or $result -notmatch 'fixture passed: elevated fresh setup') {
                throw 'Elevated setup fixture failed.'
            }
        } finally { $process.Dispose() }
    }
} finally {
    foreach ($key in @($uninstallKey, $vendorKey)) {
        if (Test-Path -LiteralPath $key) { Remove-Item -LiteralPath $key -Recurse -Force }
    }
    $actual = Get-Item -LiteralPath $lab
    if ($actual.Parent.FullName -eq $parent -and $actual.Name -cmatch '^RemovaSetupFixture-[a-f0-9]{32}$' -and
        -not ($actual.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        if (Test-Path -LiteralPath $alias) { [IO.Directory]::Delete($alias) }
        Remove-Item -LiteralPath $actual.FullName -Recurse -Force
    }
}
