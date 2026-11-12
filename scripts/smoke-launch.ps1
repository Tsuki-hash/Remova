# The release executable must render its enabled Software navigation button.
param(
    [string]$ExePath = (Join-Path (Split-Path -Parent $PSScriptRoot) 'src-tauri/target/release/remova.exe'),
    [ValidateRange(100, 60000)][int]$TimeoutMs = 30000
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance/acceptance-common.ps1"

$exe = (Get-Item -LiteralPath $ExePath).FullName
if (Get-Process -Name Remova -ErrorAction SilentlyContinue) {
    throw 'Close existing Remova processes before running the launch smoke test.'
}
$pin = [IO.File]::Open($exe, 'Open', 'Read', 'Read')
try {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $proc = Start-Process -FilePath $exe -PassThru
    try {
        if (-not (Wait-RemovaUiReady $proc $TimeoutMs)) {
            throw 'Launch smoke failed: the process did not show the enabled Software navigation button.'
        }
        if ($proc.HasExited -or $proc.MainModule.FileName -ine $exe) {
            throw 'Launch smoke failed: the running process did not match the selected executable.'
        }
        Write-Host "Launch smoke passed: application UI ready after $($timer.ElapsedMilliseconds) ms."
    } finally { Close-RemovaTestProcess $proc }
} finally { $pin.Dispose() }
