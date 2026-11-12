# Remova Release Status

## 1.4.1 (in development)

- **Status**: 1.4.1 is in development. The version remains 1.4.1. Configure the release mode before creating the candidate draft. Final acceptance and approval are required before publishing.
- **CI gate** (carried from 1.4.0): the release workflow requires a successful `CI` run (`test` and `package` both green) for the exact tagged commit, a matching `CHANGELOG.md` section for the tag, and will not replace an already-published release.
- **Signing and publication**: the current candidate uses unsigned Windows packages and manual downloads with `ALLOW_UNSIGNED_RELEASE=true`. A tag run produces a draft. Before publishing, review the exact draft attachments and SBOM. On the same Windows PC, verify WebView2 readiness with the executable from the draft portable ZIP under both a regular and an elevated token. Bind both records to the candidate commit and executable digest. See the [signing and publication guide](.github/RELEASE_SIGNING.md).
- **Updater**: in-app updates stay disabled. The upstream `tauri-plugin-updater` still depends on a `zip` version with open advisories, so update assets are not produced; upgrades remain manual downloads verified via `SHA256SUMS.txt`.
- **SBOM**: every release attaches a CycloneDX SBOM (Cargo + production npm dependencies) alongside the installers, continuing the practice introduced in 1.4.0.

## 1.4.0 (published 2026-10-25)

- **Status**: published as `v1.4.0`. Assets: NSIS setup, MSI, portable zip, `SHA256SUMS.txt`, and `SBOM.cdx.json`; unsigned, manual download.
- **Upgrade note**: 1.3.1 has no in-app update key configured — download 1.4.0 from the Release page and verify with `SHA256SUMS.txt`.
- **Known design constraint**: the destructive transaction lock remains process-wide across backup/uninstall/delete so two cleanups cannot interleave. Cancellation is exposed in the UI and honoured at backup/restore/delete phase boundaries; a running vendor uninstaller is not killed mid-flight.
- **Backups**: `%PROGRAMDATA%\Remova\Backup` is created with a private DACL (SYSTEM/Administrators + owner). Only the new session is hardened strictly; historical sessions are hardened best-effort so an unreadable legacy session cannot block new backups. History files also receive the private DACL.

This file exists so a clean checkout/tag can see the release gates even though `docs/` is gitignored.
