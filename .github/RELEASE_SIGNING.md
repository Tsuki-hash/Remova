# Release signing

Updater signing is optional but explicit. The 1.3.2 release is published
unsigned/manual by maintainer decision (see `RELEASE-STATUS.md`); the workflow sets
`ALLOW_UNSIGNED_RELEASE=true` only for `v1.3.2*` tags so an intentional unsigned
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
