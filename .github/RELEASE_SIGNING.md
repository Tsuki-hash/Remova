# Release signing

The repository configures application updates and Windows Authenticode independently.

| Release mode | Repository settings |
|---|---|
| Manual download, unsigned Windows installers | `ALLOW_UNSIGNED_RELEASE=true`, updater signing settings absent, and SignPath disabled. |
| Manual download, Authenticode-signed Windows installers | `ALLOW_UNSIGNED_RELEASE=true`, updater signing settings absent, and SignPath enabled and configured. |
| Signed application updates | `UPDATER_ENABLED=true` and all three updater signing settings. SignPath is optional. |

The legacy `v1.4.0` tag supplies its own manual-download allowance. Later tags use the repository policy above. Authenticode identifies the Windows publisher. An updater signature authenticates an update downloaded by Remova. With no Authenticode signature, Windows can show publisher or SmartScreen warnings.

## Required updater settings

In GitHub repository **Settings → Secrets and variables → Actions**, configure:

| Kind | Name | Value |
| --- | --- | --- |
| Variable | `UPDATER_ENABLED` | `true` after the update dependencies and release policy are reviewed |
| Variable | `TAURI_UPDATER_PUBLIC_KEY` | Entire contents of `updater.key.pub` |
| Secret | `TAURI_SIGNING_PRIVATE_KEY` | Entire contents of the matching `updater.key` |
| Secret | `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Non-empty password for the encrypted key |

Generate the key pair once with `pwsh -NoProfile -File scripts/new-updater-key.ps1`.
It writes the encrypted key, public key and password outside the repository to a
directory restricted to the current user and SYSTEM. Back up the key and password
securely. Do not replace the key for each release: installed clients trust the public
key embedded in their version. Do not paste private keys into issues or chat.

Leave `SIGNPATH_ENABLED` unset or set it to `false` while no usable SignPath subscription
and certificate are available. Leftover SignPath settings do not activate signing.
Updater-only releases do not require a SignPath account, API token or certificate.

## Enable SignPath later

Set repository variable `SIGNPATH_ENABLED` to exactly `true`, and configure:

| Kind | Name |
| --- | --- |
| Secret | `SIGNPATH_API_TOKEN` |
| Variable | `SIGNPATH_ORGANIZATION_ID` |
| Variable | `SIGNPATH_PROJECT_SLUG` |
| Variable | `SIGNPATH_SIGNING_POLICY_SLUG` |
| Variable | `SIGNPATH_CERTIFICATE_THUMBPRINT` |

The thumbprint is the approved certificate's 40-digit hexadecimal SHA-1 thumbprint.
Configure SignPath artifact configurations `application` and `installers` using the
XML templates in `.github/signpath/`. Production signing may require approval of
both signing requests. Confirm the provider's applicable subscription and approval policy.

When enabled, missing settings, failed signing, invalid certificates, unexpected signers
or missing timestamps stop the release. The workflow never silently falls back to
unsigned Windows binaries after SignPath has been enabled.

## Configure release approval signing

The draft publication workflow trusts the RSA public PEM in the GitHub Actions
repository variable `RELEASE_APPROVAL_PUBLIC_KEY`. The release approver keeps the
matching private PEM in a user-restricted directory outside the checkout. Only the
public PEM goes into GitHub. The approval uses RSA-PSS with SHA-256, independently
of the Windows publisher and updater signatures.

Generate a key pair in an existing private directory if no release approval key has
been provisioned. The example refuses to replace existing keys.

```powershell
$keyDir = 'D:/private/remova-release-approval'
$privateKey = Join-Path $keyDir 'approval-private.pem'
$publicKey = Join-Path $keyDir 'approval-public.pem'
if ((Test-Path -LiteralPath $privateKey) -or (Test-Path -LiteralPath $publicKey)) {
    throw 'The release approval key already exists.'
}
$rsa = [Security.Cryptography.RSA]::Create(3072)
try {
    $rsa.ExportPkcs8PrivateKeyPem() | Out-File -LiteralPath $privateKey -Encoding utf8NoBOM -NoClobber
    $rsa.ExportSubjectPublicKeyInfoPem() | Out-File -LiteralPath $publicKey -Encoding utf8NoBOM -NoClobber
} finally {
    $rsa.Dispose()
}
```

Set `RELEASE_APPROVAL_PUBLIC_KEY` to the entire contents of `approval-public.pem`.
The workflow stops if this variable is missing or the signature cannot be verified.

## Release order

1. Validate signing configuration, version and tests.
2. Build the application with the updater public key embedded.
3. If enabled, sign and verify the application through SignPath.
4. Bundle the application into NSIS and MSI installers.
5. If enabled, sign and verify the final installers through SignPath.
6. Generate Tauri signatures for the final installers and verify them against the
   same public key embedded in the application. Generate `latest.json` only after
   both installers pass verification.
7. Complete smoke checks and portable packaging. The tag workflow uploads the packages, checksums, SBOM and any updater assets to a draft GitHub Release.
8. Download the draft attachments into one folder and run the [isolated acceptance suite](../scripts/acceptance/README.md) on them. Keep the four summaries with their evidence files. Review the package hashes, SBOM, applicable license notices, publication mode and [recovery guide](../RECOVERY.md).
9. Run the [performance gate](../benchmarks/performance-thresholds.md) against `remova.exe` from that portable ZIP. Keep the cold-start and component measurement JSON. Produce the signed approval input using the approved tag, exact commit and four acceptance summaries.

   ```powershell
   $summaries = @(
       'D:/acceptance/win10-client/acceptance-summary.json'
       'D:/acceptance/win10-elevated/acceptance-summary.json'
       'D:/acceptance/win11-client/acceptance-summary.json'
       'D:/acceptance/win11-elevated/acceptance-summary.json'
   )
   $approval = & ./scripts/release-approval.ps1 -Tag v1.4.1 -Commit $commit `
       -BundleDir 'D:/candidate' -Acceptance $summaries `
       -Startup 'D:/acceptance/performance/startup-cold.json' `
       -Components 'D:/acceptance/performance/components.json' `
       -PrivateKeyPath 'D:/private/remova-release-approval/approval-private.pem' -Reviewed
   gh workflow run publish-release.yml --ref main -f tag=v1.4.1 -f "approval=$approval"
   ```

   A repository maintainer or administrator dispatches **Publish release draft** from `main`. The job checks the signature, tag, attachment bytes and executable inside the downloaded portable ZIP before publishing. The signed payload contains the tag, commit and digests of the attachments and reviewed evidence. Keep the original evidence and observations with the private acceptance records.

The NSIS update manifest uses the custom target `windows-x86_64-nsis`.
MSI and portable users retain manual updates. Existing releases cannot be overwritten.
No tags, builds or releases are triggered merely by editing these files.

Run `pwsh -NoProfile -File scripts/test-release-signing-policy.ps1` to check policy
branches without compiling or packaging. The test uses temporary fixture files and
stubbed signing commands; cryptographic signature validation is covered separately
by `scripts/test-updater-signature.ps1`.

## Release incident procedure / 发版事故程序

EN: If a published release is found broken or malicious:

1. **Pull the release**: delete the GitHub Release (keep the tag only if the
   artifact itself is fine and the problem is metadata) or delete both the
   release and the tag when the build must not be downloaded at all.
2. **Announce**: pin an issue describing what is broken, which versions are
   affected, and what users of each channel (NSIS / MSI / portable) should do.
3. **Checksums**: state explicitly whether the published `SHA256SUMS.txt` is
   still trustworthy for unaffected artifacts; if the release was replaced,
   regenerate and re-publish it with the new build.
4. **Downgrade guidance**: users may install an older release from the same
   Releases page. Remova does not ship an automatic rollback for the app
   itself — state the last known-good version and link it.
5. **Post-mortem**: record the cause in the next release's notes when it is
   user-relevant.

中文：已发布版本发现损坏或被污染时的处置程序——

1. **撤下发布**：删除 GitHub Release（若问题仅在元数据可保留 tag；构建本身
   不可下载时连 tag 一并删除）。
2. **公告**：置顶 issue 说明损坏内容、受影响版本，以及 NSIS / MSI / 便携包
   三类用户各自应采取的动作。
3. **校验和口径**：明确说明 `SHA256SUMS.txt` 对未受影响产物是否仍然可信；
   若重新发布，需随新构建重新生成并上传。
4. **降级指引**：用户可在 Releases 页下载旧版本。应用本身没有自动回滚——
   公告中给出最后已知可用版本并附链接。
5. **复盘**：与用户相关的结论写入下一个版本的发布说明。
