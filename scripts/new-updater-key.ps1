# Run once locally. Never run this per release or store these files in the repository.
[CmdletBinding()]
param([string]$KeyDirectory = (Join-Path $env:LOCALAPPDATA 'Remova/release-signing'))
$ErrorActionPreference = 'Stop'
$KeyDirectory = [IO.Path]::GetFullPath($KeyDirectory)
if (Test-Path -LiteralPath $KeyDirectory) { throw "Key directory already exists; refusing to replace keys: $KeyDirectory" }
New-Item -ItemType Directory -Path $KeyDirectory | Out-Null
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
& icacls $KeyDirectory /inheritance:r /grant:r "${sid}:(OI)(CI)F" '*S-1-5-18:(OI)(CI)F' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not protect the signing key directory.' }
$password = [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))
$key = Join-Path $KeyDirectory 'updater.key'
# Capture CLI output because signing tools may print key material.
$output = & npx tauri signer generate --ci --password $password --write-keys $key 2>&1
if ($LASTEXITCODE -ne 0) { throw 'Updater key generation failed; inspect the protected key directory before retrying.' }
if (!(Test-Path -LiteralPath $key) -or !(Test-Path -LiteralPath "$key.pub")) { throw 'Signing tool did not produce the key pair.' }
[IO.File]::WriteAllText((Join-Path $KeyDirectory 'password.txt'), $password)
Write-Host "Keys saved in protected directory: $KeyDirectory"
Write-Host 'Back up the key and password securely. Upload them to GitHub Secrets; upload updater.key.pub to the TAURI_UPDATER_PUBLIC_KEY repository variable. Do not paste private material into chat.'
