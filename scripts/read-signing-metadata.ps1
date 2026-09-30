param([Parameter(Mandatory)][string]$Path, [string]$Prefix = '')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw | ConvertFrom-Json).version
$metadata = [Diagnostics.FileVersionInfo]::GetVersionInfo((Resolve-Path -LiteralPath $Path).Path)
if ($metadata.ProductName -cne 'Remova' -or $metadata.ProductVersion -notin @($version, "$version.0")) {
    throw "Unexpected product metadata: $Path"
}
"${Prefix}product-version=$($metadata.ProductVersion)" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
"${Prefix}file-version=$($metadata.FileVersion)" | Add-Content -LiteralPath $env:GITHUB_OUTPUT -Encoding utf8
