# Remova Release Status

## 1.5.2 (published 2026-11-23)

- **Status**: [`v1.5.2`](https://github.com/Tsuki-hash/Remova/releases/tag/v1.5.2) is the latest public release. The manual-download attachments are the unsigned NSIS setup, MSI, portable ZIP, `SHA256SUMS.txt`, and `SBOM.cdx.json`. GitHub reports a SHA-256 digest for each attachment.
- **Validation**: the tag resolves to `feef8e3a7ae10cacbafa1983eaaa617c5cc5a126`. The exact commit's [CI test and package jobs](https://github.com/Tsuki-hash/Remova/actions/runs/45098650324) and the [publication approval run](https://github.com/Tsuki-hash/Remova/actions/runs/45106684138) passed. This is historical evidence for 1.5.2, not for the next candidate.
- **Downloads**: verify an installer or portable package against `SHA256SUMS.txt` from the same release before using it. The [recovery guide](RECOVERY.md) covers upgrades, downgrades, and retaining local data.

## 1.5.1 (published 2026-11-17)

- **Status**: published as [`v1.5.1`](https://github.com/Tsuki-hash/Remova/releases/tag/v1.5.1) with unsigned NSIS, MSI and portable packages, `SHA256SUMS.txt`, and `SBOM.cdx.json`.
- **Validation**: the tag resolves to `fc9c44d01732a5d21c0f774a208ff61cbee96026`. The exact candidate's [CI](https://github.com/Tsuki-hash/Remova/actions/runs/44058796950) and [release build](https://github.com/Tsuki-hash/Remova/actions/runs/44062070949) passed. The same executable from the draft portable ZIP reached WebView2 navigation readiness on Windows 11 x64 under both regular and administrator tokens. All five published asset digests match the reviewed attachments; the locked-dependency SBOM and bundled license notices were verified.
- **Publication**: the [approval workflow](https://github.com/Tsuki-hash/Remova/actions/runs/44070947452) passed and verified the title, notes, attachments and latest release at publication. Its tooling passed a separate [CI run](https://github.com/Tsuki-hash/Remova/actions/runs/44065380049).
- **CI gate** (carried from 1.4.0): the release workflow requires a successful `CI` run (`test` and `package` both green) for the exact tagged commit, a matching `CHANGELOG.md` section for the tag, and will not replace an already-published release.
- **Signing and publication**: distribution uses unsigned Windows packages and manual downloads with `ALLOW_UNSIGNED_RELEASE=true`. A tag run produces a draft. Before publishing, review the exact draft attachments and SBOM. On the same Windows PC, verify WebView2 readiness with the executable from the draft portable ZIP under both a regular and an elevated token. Bind both records to the candidate commit and executable digest. See the [signing and publication guide](.github/RELEASE_SIGNING.md).
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
