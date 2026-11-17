# Build the locked Windows bundle and record the tools and source inputs.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$EvidenceFile)

$ErrorActionPreference = 'Stop'
$evidence = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($EvidenceFile)
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function Invoke-BuildTool([string]$Tool, [string[]]$Arguments) {
    $result = & $Tool @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Tool exited $LASTEXITCODE." }
    $result
}

Push-Location $root
try {
    $commit = Invoke-BuildTool git @('rev-parse', 'HEAD')
    if (Invoke-BuildTool git @('status', '--porcelain', '--untracked-files=no')) {
        throw 'Build the candidate from a clean checkout of the target commit.'
    }
    $inputs = [ordered]@{}
    foreach ($file in 'package-lock.json', 'src-tauri/Cargo.lock', 'src-tauri/tauri.conf.json', 'src-tauri/windows/installer.nsi', 'src-tauri/windows/installer-hooks.nsh') {
        $inputs[$file] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $tools = [ordered]@{
        windows = [Environment]::OSVersion.VersionString
        runner_os = $env:RUNNER_OS
        runner_arch = $env:RUNNER_ARCH
        runner_image = $env:ImageOS
        runner_image_version = $env:ImageVersion
        node = Invoke-BuildTool node @('--version')
        npm = Invoke-BuildTool npm @('--version')
        rustc = @(Invoke-BuildTool rustc @('-Vv'))
        cargo = Invoke-BuildTool cargo @('--version')
        tauri = Invoke-BuildTool npm @('exec', '--offline', '--', 'tauri', '--version')
    }
    Invoke-BuildTool npm @('exec', '--offline', '--', 'tauri', 'build', '--', '--locked')
    foreach ($file in $inputs.Keys) {
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ine $inputs[$file]) {
            throw "The build changed $file. Updated inputs must be committed before packaging."
        }
    }
    $tauriTools = Join-Path $env:LOCALAPPDATA 'tauri'
    $tools.nsis = Invoke-BuildTool (Join-Path $tauriTools 'NSIS/makensis.exe') @('/VERSION')
    $tools.wix = [ordered]@{}
    foreach ($exe in 'candle.exe', 'light.exe') {
        $tools.wix[$exe] = (Get-Item -LiteralPath (Join-Path $tauriTools "WixTools314/$exe")).VersionInfo.FileVersion
    }
    [ordered]@{ schema_version = 1; commit = $commit; tools = $tools; inputs_sha256 = $inputs } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $evidence -Encoding utf8
    Write-Host "Wrote build inputs and tool versions to $evidence."
} finally { Pop-Location }
