# Release signing

Updater signing is optional but explicit. The 1.4.0 release is published
unsigned/manual by maintainer decision (see `RELEASE-STATUS.md`); the workflow sets
`ALLOW_UNSIGNED_RELEASE=true` only for the exact `v1.4.0` tag so an intentional unsigned
release is distinguishable from a missing-key misconfiguration. Later releases need
an explicit `ALLOW_UNSIGNED_RELEASE=true` repository variable; the bypass is not
inherited. When updater keys are configured, every stable
release must carry Tauri updater signatures. Windows Authenticode signing through
SignPath is optional and disabled by default. These signatures serve different
purposes: the updater signature authenticates the downloaded update; Authenticode
identifies the Windows publisher. Without Authenticode, Windows may show unknown-publisher
or SmartScreen warnings even though the updater verifies the download.

## Required updater settings

In GitHub repository **Settings → Secrets and variables → Actions**, configure:

| Kind | Name | Value |
| --- | --- | --- |
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

## Release order

1. Validate signing configuration, version and tests.
2. Build the application with the updater public key embedded.
3. If enabled, sign and verify the application through SignPath.
4. Bundle the application into NSIS and MSI installers.
5. If enabled, sign and verify the final installers through SignPath.
6. Generate Tauri signatures for the final installers and verify them against the
   same public key embedded in the application. Generate `latest.json` only after
   both installers pass verification.
7. Complete smoke checks and portable packaging. Upload artifacts into a draft
   GitHub Release, then publish it once the upload succeeds.

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
