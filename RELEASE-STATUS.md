# Remova Release Status

## 1.4.1 (in development)

- **Status**: planning the next cycle — stability follow-up from 1.4.0 feedback, usability and UI refinements of existing features, and a bounded set of new capabilities. Scope and the final version number are decided at scope freeze: if new capabilities ship beyond patch semantics, the version becomes 1.5.0 per the X.Y.0 feature-release convention.
- **CI gate** (carried from 1.4.0): the release workflow requires a successful `CI` run (`test` and `package` both green) for the exact tagged commit, a matching `CHANGELOG.md` section for the tag, and will not replace an already-published release.
- **Signing**: the unsigned-release exception was scoped to the exact `v1.4.0` tag. Any later release needs either the `ALLOW_UNSIGNED_RELEASE=true` repository variable or configured signing (SignPath variables are reserved in the workflow). This decision must be made before tagging.
- **Updater**: in-app updates stay disabled. The upstream `tauri-plugin-updater` still depends on a `zip` version with open advisories, so update assets are not produced; upgrades remain manual downloads verified via `SHA256SUMS.txt`.
- **SBOM**: every release attaches a CycloneDX SBOM (Cargo + production npm dependencies) alongside the installers, continuing the practice introduced in 1.4.0.

## 1.4.0 (published 2026-10-25)

- **Status**: published as `v1.4.0`. Assets: NSIS setup, MSI, portable zip, `SHA256SUMS.txt`, and `sbom.cdx.json`; unsigned, manual download.
- **Upgrade note**: 1.3.1 has no in-app update key configured — download 1.4.0 from the Release page and verify with `SHA256SUMS.txt`.
- **Known design constraint**: the destructive transaction lock remains process-wide across backup/uninstall/delete so two cleanups cannot interleave. Cancellation is exposed in the UI and honoured at backup/restore/delete phase boundaries; a running vendor uninstaller is not killed mid-flight.
- **Backups**: `%PROGRAMDATA%\Remova\Backup` is created with a private DACL (SYSTEM/Administrators + owner). Only the new session is hardened strictly; historical sessions are hardened best-effort so an unreadable legacy session cannot block new backups. History files also receive the private DACL.

This file exists so a clean checkout/tag can see the release gates even though `docs/` is gitignored.
