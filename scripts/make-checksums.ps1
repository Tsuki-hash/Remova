# Generate SHA256SUMS.txt for release artifacts (same format as v1.3.1).
param(
    [string]$BundleRoot = (Join-Path (Split-Path -Parent $PSScriptRoot) "src-tauri\target\release\bundle"),
    [string]$Output = ""
)
$ErrorActionPreference = "Stop"
$patterns = @("nsis\*-setup.exe", "msi\*.msi", "portable\*.zip")
$files = @()
foreach ($pattern in $patterns) {
    $full = Join-Path $BundleRoot $pattern
    $files += @(Get-ChildItem -Path $full -File -ErrorAction SilentlyContinue)
}
if ($files.Count -eq 0) {
    throw "No release artifacts found under $BundleRoot."
}
$lines = $files |
    Sort-Object Name |
    ForEach-Object {
        $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $($_.Name)"
    }
$target = if ($Output) { $Output } else { Join-Path $BundleRoot "SHA256SUMS.txt" }
Set-Content -LiteralPath $target -Value $lines -Encoding ascii
Write-Host "Wrote $target ($($files.Count) artifacts)"
