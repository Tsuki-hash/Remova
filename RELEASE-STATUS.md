# Remova Release Status

## 1.5.1 (in development)

- **Status**: 1.5.1 is in development. Configure the release mode before creating a candidate draft. Final acceptance and approval are required before publishing.
- **CI gate** (carried from 1.4.0): the release workflow requires a successful `CI` run (`test` and `package` both green) for the exact tagged commit, a matching `CHANGELOG.md` section for the tag, and will not replace an already-published release.
- **Signing and publication**: the planned distribution uses unsigned Windows packages and manual downloads with `ALLOW_UNSIGNED_RELEASE=true`. A tag run produces a draft. Before publishing, review the exact draft attachments and SBOM. On the same Windows PC, verify WebView2 readiness with the executable from the draft portable ZIP under both a regular and an elevated token. Bind both records to the candidate commit and executable digest. See the [signing and publication guide](.github/RELEASE_SIGNING.md).
- **Local build fallback**: when the tag workflow cannot build the draft, use the locked local build and record its toolchain, then upload and review the same five attachments. [Local Windows release procedure](.github/LOCAL_RELEASE.md)
- **Updater**: verify manual downloads via `SHA256SUMS.txt`. The Windows updater dependency supports installer files and is built without the legacy ZIP updater feature.
- **SBOM**: every release attaches a CycloneDX SBOM (Cargo + production npm dependencies) alongside the installers, continuing the practice introduced in 1.4.0.

## 1.5.0 (published 2026-11-16)

- **Status**: published as [`v1.5.0`](https://github.com/Tsuki-hash/Remova/releases/tag/v1.5.0) with unsigned NSIS, MSI and portable packages, the SHA-256 checksums and the CycloneDX SBOM.

## 1.4.0 (published 2026-10-25)

- **Status**: published as `v1.4.0`. Assets: NSIS setup, MSI, portable zip, `SHA256SUMS.txt`, and `SBOM.cdx.json`; unsigned, manual download.
- **Upgrade note**: 1.3.1 has no in-app update key configured — download 1.4.0 from the Release page and verify with `SHA256SUMS.txt`.
- **Known design constraint**: the destructive transaction lock remains process-wide across backup/uninstall/delete so two cleanups cannot interleave. Cancellation is exposed in the UI and honoured at backup/restore/delete phase boundaries; a running vendor uninstaller is not killed mid-flight.
- **Backups**: `%PROGRAMDATA%\Remova\Backup` is created with a private DACL (SYSTEM/Administrators + owner). Only the new session is hardened strictly; historical sessions are hardened best-effort so an unreadable legacy session cannot block new backups. History files also receive the private DACL.

This file exists so a clean checkout/tag can see the release gates even though `docs/` is gitignored.
