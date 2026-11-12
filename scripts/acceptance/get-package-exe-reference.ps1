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
$root = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')
$distributionHashes = [ordered]@{}
foreach ($name in @('LICENSE', 'THIRD-PARTY-NOTICES.md')) {
    $distributionHashes[$name] = (Get-FileHash -LiteralPath (Join-Path $root.Path $name) -Algorithm SHA256).Hash.ToLowerInvariant()
}
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
        $names = @('remova.exe') + @($distributionHashes.Keys)
        if ($Channel -ceq 'nsis') {
            $include = $names | ForEach-Object { "-ir!$_" }
            & $SevenZip e "-o$out" -aou $Package @include | Out-Null
            if ($LASTEXITCODE -ne 0) { throw '7-Zip could not read the NSIS installer.' }
            $files = @(Get-ChildItem -LiteralPath $out -File)
            if ($files.Count -ne $names.Count) {
                throw 'The NSIS installer must contain one copy of the executable and each license file.'
            }
        } else {
            $wxs = Join-Path $out 'package.wxs'
            & $WixDark -nologo -x $out $Package -o $wxs | Out-Null
            if ($LASTEXITCODE -ne 0) { throw 'WiX dark could not read the MSI installer.' }
            [xml]$wix = Get-Content -Raw -LiteralPath $wxs
            $files = @($wix.SelectNodes("//*[local-name()='File']") | Where-Object { $_.Name -iin $names })
        }
        $hashes = @{}
        foreach ($name in $names) {
            $entries = @($files | Where-Object { $_.Name -ieq $name })
            if ($entries.Count -ne 1) { throw "The $Channel installer must contain one $name payload." }
            if ($Channel -ceq 'msi') {
                $source = $entries[0].GetAttribute('Source')
                if (-not [IO.Path]::IsPathFullyQualified($source)) { $source = Join-Path $out $source }
                $file = Get-Item -LiteralPath $source
            } else {
                $file = $entries[0]
            }
            if (-not $file.FullName.StartsWith($out + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                throw "The $Channel payload did not resolve within the extraction directory: $name"
            }
            if ($file.PSIsContainer -or !$file.Length -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw "Invalid $Channel payload: $name"
            }
            if ($name -ceq 'remova.exe' -and -not (Test-AcceptanceExecutableVersion $file.VersionInfo.ProductVersion $CandidateVersion)) {
                throw "The $Channel payload does not report the candidate version."
            }
            $hashes[$name] = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($distributionHashes.Contains($name) -and $hashes[$name] -cne $distributionHashes[$name]) {
                throw "The $Channel installer contains unexpected contents for $name."
            }
        }
        $hashes['remova.exe']
    } finally { $pin.Dispose() }
}

try {
    $reference = [ordered]@{}
    foreach ($channel in $identity.artifacts.Keys) {
        $artifact = $identity.artifacts[$channel]
        $path = (Resolve-Path -LiteralPath (Join-Path $Artifacts $artifact.name)).Path
        $exeHash = if ($channel -ceq 'portable') { Get-PortableExeHash $path $artifact.sha256 $distributionHashes }
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
