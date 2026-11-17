# Build an unsigned Windows release locally

Use this procedure when the tag workflow cannot create a draft. The local build produces the same five manual-download attachments. The draft must still pass the exact-commit CI check and the existing approval workflow.

## Build and stage the candidate

Use Windows x64, PowerShell 7, the Visual Studio C++ desktop build tools, WebView2, and 7-Zip. Use the Node and Rust versions in the CI `package` job. The writable output directory must be outside the checkout. The package build records input hashes, the SHA-256 digests of the EXE and both installers, tool versions, and the commit in `windows-build.json`.

```powershell
$commit = '<40-character candidate commit>'
$tag = '<vX.Y.Z>'
$src = 'D:/release/candidate-src'
$record = 'D:/release/windows-build.json'
$stage = 'D:/release/candidate'
$sevenZip = 'C:/Program Files/7-Zip/7z.exe'
git worktree add --detach $src $commit
Set-Location $src
$env:RUSTUP_TOOLCHAIN = '1.98.0'
node --version
npm ci
./scripts/build-windows-candidate.ps1 -EvidenceFile $record
./scripts/stage-local-release.ps1 -Tag $tag -BuildEvidence $record -OutDir $stage -SevenZip $sevenZip
```

Stop if any command fails. The staging command requires the current build-record schema, checks every recorded input and output digest, holds the output files open against replacement, and checks the staged packages, release notes, checksums, payloads, and included licenses. The `attachments` directory contains NSIS, MSI, the portable ZIP, SHA-256 checksums, and the CycloneDX SBOM. Keep `build-inputs.json` and `package-exe-reference.json` with the acceptance records. You can rehearse these steps with a local commit before a release tag exists.

## Upload and review the draft

Use the existing remote tag and confirm it resolves to the built commit. The CI gate requires `test` and `package` to pass for that exact commit on `main`.

```powershell
$repo = 'Tsuki-hash/Remova'
$remoteCommit = gh api "repos/$repo/commits/refs/tags/$tag" --jq '.sha'
if ($LASTEXITCODE -or $remoteCommit -cne $commit) { throw 'The tag does not identify the local build.' }
./scripts/require-release-ci.ps1 -Repository $repo -Commit $commit
$version = $tag.Substring(1)
$assets = @(Get-ChildItem -LiteralPath "$stage/attachments" -File | ForEach-Object FullName)
gh release create $tag @assets --repo $repo --verify-tag --draft --title "Remova $version" --notes-file "$stage/release-notes.md"
if ($LASTEXITCODE) { throw 'Stop and inspect the existing draft or upload error.' }
```

When a draft already exists, use its downloaded attachments for review and leave its contents in place. Download from GitHub into a new directory, compare the attachment names and SHA-256 digests against the staged files, and confirm the draft title and notes. Continue from the GitHub copy. The signing and publication workflow checks the downloaded attachments again. [Release signing and publication guide](RELEASE_SIGNING.md)

Publication verifies the public tag, metadata, assets, and the latest-release endpoint. An approval with `make_latest` set to `true` must point latest to the published release. With `false`, the previous latest identity is recorded before publication and must remain the same. Read-back failure requires inspecting the public state before retrying. The verification record includes both identities and the approved setting.

Extract the final portable ZIP. On the same Windows PC, use `record-launch.ps1` once from a regular PowerShell and once from an administrator PowerShell. Both observations must use this executable and its recorded SHA-256. Stop at the unpublished draft until both launch records and the attachments have been reviewed and approved. [Windows launch records](../scripts/acceptance/README.md)
