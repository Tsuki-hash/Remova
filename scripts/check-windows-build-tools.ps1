$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$expectedNode = (Get-Content -LiteralPath (Join-Path $root '.node-version') -Raw).Trim()
$toolchain = Get-Content -LiteralPath (Join-Path $root 'rust-toolchain.toml') -Raw
if ($toolchain -notmatch '(?m)^channel = "(\d+\.\d+\.\d+)"') {
    throw 'Expected an exact Rust version in rust-toolchain.toml.'
}
$expectedRust = $Matches[1]
Push-Location $root
try {
    $node = & node --version
    if ($LASTEXITCODE -or $node -cne "v$expectedNode") {
        throw "Expected Node v$expectedNode, found $node."
    }
    $rust = & rustc --version
    if ($LASTEXITCODE -or ($rust -split ' ')[1] -cne $expectedRust) {
        throw "Expected Rust $expectedRust, found $rust."
    }
    Write-Host "Windows build tools: $node, $rust"
} finally {
    Pop-Location
}
