param([Parameter(Mandatory)][string[]]$Paths)
$ErrorActionPreference = 'Stop'
if ($env:SIGNPATH_CERTIFICATE_THUMBPRINT -notmatch '^[A-Fa-f0-9]{40}$') { throw 'Missing or invalid expected signing certificate thumbprint.' }
foreach ($path in $Paths) {
    $resolved = (Resolve-Path -LiteralPath $path).Path
    $signature = Get-AuthenticodeSignature -LiteralPath $resolved
    if ($signature.Status -ne 'Valid' -or !$signature.SignerCertificate -or
        $signature.SignerCertificate.Thumbprint -ne $env:SIGNPATH_CERTIFICATE_THUMBPRINT) {
        throw "Invalid Authenticode signature or unexpected signer: $resolved ($($signature.Status))"
    }
    if (!$signature.TimeStamperCertificate) { throw "Missing trusted signing timestamp: $resolved" }
    Write-Host "Verified approved signer and timestamp: $resolved"
}
