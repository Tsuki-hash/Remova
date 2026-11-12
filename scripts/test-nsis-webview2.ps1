# Exercise the same WebView2 launch block that is compiled into the installer.
[CmdletBinding()]
param([string]$MakeNsis = (Join-Path $env:LOCALAPPDATA 'tauri/NSIS/makensis.exe'))

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$config = Get-Content -Raw -LiteralPath (Join-Path $root 'src-tauri/tauri.conf.json') | ConvertFrom-Json
$template = Get-Content -Raw -LiteralPath (Join-Path (Join-Path $root 'src-tauri') $config.bundle.windows.nsis.template)
$block = [regex]::Match($template, '(?ms)^ +install_webview2:\r?\n.*?^ +webview2_done:\s*$')
if (!$block.Success) { throw 'The configured NSIS template is missing its WebView2 launch block.' }
if ($config.bundle.windows.webviewInstallMode.type -cne 'embedBootstrapper' -or
    $template -notmatch '(?m)^ +File "/oname=\$PLUGINSDIR\\MicrosoftEdgeWebview2Setup\.exe"[^\r\n]*\r?\n +IfErrors webview2_launch_failed') {
    throw 'The embedded WebView2 payload must be extracted privately and checked before launch.'
}
$MakeNsis = (Resolve-Path -LiteralPath $MakeNsis).Path

$parent = (Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path
$fixture = (New-Item -ItemType Directory -Path (Join-Path $parent ('remova nsis fixture ' + [guid]::NewGuid().ToString('N')))).FullName
try {
    $child = @'
Unicode true
Name "WebView2 child fixture"
OutFile "${OUTPUT}"
RequestExecutionLevel user
SilentInstall silent
Section
  SetErrorLevel ${EXIT_CODE}
  Quit
SectionEnd
'@
    $installer = @'
Unicode true
!include "LogicLib.nsh"
!define WEBVIEW2INSTALLERARGS ""
Name "WebView2 launch fixture"
OutFile "${OUTPUT}"
RequestExecutionLevel user
SilentInstall silent
LangString installingWebview2 1033 "Starting"
LangString webview2InstallSuccess 1033 "Started successfully"
LangString webview2InstallError 1033 "Child exit code $1"
LangString removaWebView2StartError 1033 "Unable to start"
LangString webview2AbortError 1033 "WebView2 launch failed"
Section
  StrCpy $1 0
  StrCpy $6 "${CHILD}"
  SetErrors
  Goto install_webview2
@LAUNCH_BLOCK@
  FileOpen $0 "${MARKER}" w
  FileWrite $0 "continued"
  FileClose $0
SectionEnd
'@
    $childScript = Join-Path $fixture 'child.nsi'
    $installerScript = Join-Path $fixture 'fixture.nsi'
    $child | Set-Content -LiteralPath $childScript -Encoding utf8
    $installer.Replace('@LAUNCH_BLOCK@', $block.Value) | Set-Content -LiteralPath $installerScript -Encoding utf8

    $cases = @(
        @{ name = 'missing child'; childExit = $null; expectedExit = 2 },
        @{ name = 'nonzero exit'; childExit = 23; expectedExit = 2 },
        @{ name = 'successful exit'; childExit = 0; expectedExit = 0 }
    )
    foreach ($case in $cases) {
        $executable = Join-Path $fixture "$($case.name) child.exe"
        if ($null -ne $case.childExit) {
            & $MakeNsis /V2 "/DOUTPUT=$executable" "/DEXIT_CODE=$($case.childExit)" $childScript
            if ($LASTEXITCODE -ne 0) { throw "Could not compile the child fixture for $($case.name)." }
        }
        $probe = Join-Path $fixture "$($case.name) fixture.exe"
        $marker = Join-Path $fixture "$($case.name) continued.txt"
        & $MakeNsis /V2 "/DOUTPUT=$probe" "/DCHILD=$executable" "/DMARKER=$marker" $installerScript
        if ($LASTEXITCODE -ne 0) { throw "Could not compile the launch fixture for $($case.name)." }
        $process = Start-Process -FilePath $probe -Wait -PassThru -WindowStyle Hidden
        try {
            if ($process.ExitCode -ne $case.expectedExit -or
                (Test-Path -LiteralPath $marker -PathType Leaf) -ne ($case.expectedExit -eq 0)) {
                throw "$($case.name) returned exit $($process.ExitCode) or continued unexpectedly."
            }
        } finally { $process.Dispose() }
        Write-Host "WebView2 fixture passed: $($case.name)"
    }
} finally {
    $actual = Get-Item -LiteralPath $fixture
    if ($actual.Parent.FullName -eq $parent -and $actual.Name -cmatch '^remova nsis fixture [a-f0-9]{32}$' -and
        -not ($actual.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        Remove-Item -LiteralPath $actual.FullName -Recurse -Force
    }
}
