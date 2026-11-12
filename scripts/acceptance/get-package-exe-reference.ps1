#!/usr/bin/env pwsh
# Read the final candidate payloads without launching or installing them.
param(
    [Parameter(Mandatory)][string]$Artifacts,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$CandidateVersion,
    [Parameter(Mandatory)][string]$SevenZip,
    [Parameter(Mandatory)][string]$WixDark
)

$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/acceptance-common.ps1"
$identity = Get-AcceptanceArtifacts $Artifacts $CandidateVersion
if (@($identity.checks | Where-Object { $_.status -ne 'pass' }).Count) {
    throw 'The candidate packages do not match the release checksums.'
}
$SevenZip = (Resolve-Path -LiteralPath $SevenZip).Path
$WixDark = (Resolve-Path -LiteralPath $WixDark).Path
$base = (Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())).Path
$scratch = Join-Path $base ('remova-package-exe-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch | Out-Null

function Get-InstalledPackageExeHash([string]$Channel, [string]$Package, [string]$ExpectedHash) {
    $pin = [IO.File]::Open($Package, 'Open', 'Read', 'Read')
    try {
        if ([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($pin)).ToLowerInvariant() -cne $ExpectedHash) {
            throw "The $Channel installer changed before extraction."
        }
        $out = (New-Item -ItemType Directory -Path (Join-Path $scratch $Channel)).FullName
        if ($Channel -ceq 'nsis') {
            & $SevenZip e "-o$out" -aou $Package '-ir!remova.exe' | Out-Null
            if ($LASTEXITCODE -ne 0) { throw '7-Zip could not read the NSIS installer.' }
            $files = @(Get-ChildItem -LiteralPath $out -File)
            if ($files.Count -ne 1 -or $files[0].Name -ine 'remova.exe') {
                throw 'The NSIS installer must contain one remova.exe payload.'
            }
            $file = $files[0]
        } else {
            $wxs = Join-Path $out 'package.wxs'
            & $WixDark -nologo -x $out $Package -o $wxs | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'WiX dark could not read the MSI installer.' }
            [xml]$wix = Get-Content -Raw -LiteralPath $wxs
            $payloadFiles = @($wix.SelectNodes("//*[local-name()='File']") | Where-Object { $_.Name -ieq 'remova.exe' })
            if ($payloadFiles.Count -ne 1) { throw 'The MSI file table must identify one remova.exe payload.' }
            $source = $payloadFiles[0].GetAttribute('Source')
            if (-not [IO.Path]::IsPathFullyQualified($source)) { $source = Join-Path $out $source }
            $file = Get-Item -LiteralPath $source
            if (-not $file.FullName.StartsWith($out + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                throw 'The MSI executable did not resolve within the extraction directory.'
            }
        }
        if ($file.PSIsContainer -or !$file.Length -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Invalid $Channel executable payload."
        }
        if (-not (Test-AcceptanceExecutableVersion $file.VersionInfo.ProductVersion $CandidateVersion)) {
            throw "The $Channel payload does not report the candidate version."
        }
        (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    } finally { $pin.Dispose() }
}

try {
    $reference = [ordered]@{}
    foreach ($channel in $identity.artifacts.Keys) {
        $artifact = $identity.artifacts[$channel]
        $path = (Resolve-Path -LiteralPath (Join-Path $Artifacts $artifact.name)).Path
        $exeHash = if ($channel -ceq 'portable') { Get-PortableExeHash $path $artifact.sha256 }
            else { Get-InstalledPackageExeHash $channel $path $artifact.sha256 }
        $reference[$channel] = @{ package_sha256 = $artifact.sha256; executable_sha256 = $exeHash }
    }
    $reference | ConvertTo-Json -Depth 3
} finally {
    $actual = Get-Item -LiteralPath $scratch
    if ($actual.Parent.FullName -eq $base -and $actual.Name -cmatch '^remova-package-exe-[a-f0-9]{32}$' -and
        -not ($actual.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        Remove-Item -LiteralPath $actual.FullName -Recurse -Force
    }
}
