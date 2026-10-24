# Remova Release Status

## 1.4.0

- **Status**: code complete; real-machine acceptance (A1–A10 / A12–A13) remains before tagging. The tagged commit must be the current `main` tip and have a successful `CI` run.
- **Version**: 1.4.0 — renumbered from 1.3.2 on 2026-10-24 because the cycle ships new capabilities (minor bump per the X.Y.0 feature-release convention). Release content is unchanged.
- **CI gate**: the release workflow requires a successful `CI` run (`test` and `package` both green) for the exact tagged commit. Any commit after a verified candidate needs its own green run before tagging.
- **Real-machine acceptance**: A1–A10 / A12–A13 are executed by the maintainer in an isolated Windows 10/11 environment before release. Evidence is kept in `docs/product/acceptance/ACCEPTANCE-1.4.0.md` (docs are local-only and are not part of the tag).
- **Signing / update chain**: waived by the maintainer on 2026-10-22. Release artifacts are unsigned and upgrades are manual; `latest.json`/`.sig` are produced only when updater keys are configured. The unsigned path is allowed only for the exact `v1.4.0` tag; later releases need an explicit `ALLOW_UNSIGNED_RELEASE=true` repository variable.
- **Backups**: `%PROGRAMDATA%\Remova\Backup` is created with a private DACL (SYSTEM/Administrators + owner). Only the new session is hardened strictly; historical sessions are hardened best-effort so an unreadable legacy session cannot block new backups. History files also receive the private DACL.
- **Known design constraint**: the destructive transaction lock remains process-wide across backup/uninstall/delete so two cleanups cannot interleave. Cancellation is exposed in the UI and honoured at backup/restore/delete phase boundaries; a running vendor uninstaller is not killed mid-flight.
- **Verification at the current candidate**: Rust lib 570 passed / 0 failed / 7 ignored; frontend 69 files / 525 tests; coverage 88.34 / 81.27 / 86.42 / 90.75; `cargo fmt` / `clippy -D warnings`, lint, build, i18n (744 keys / 0 dead), internal-identifier check, scripts 18/18, commands 69/69, five-source version alignment and docs index all green.

This file exists so a clean checkout/tag can see the release gates even though `docs/` is gitignored.
