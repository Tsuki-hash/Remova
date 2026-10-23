# Remova Release Status

## 1.3.2

- **Candidate**: the round-46 fix set on `main` (code through `deaa147`; a docs-only commit may follow). The final candidate must be a commit containing these fixes and must pass CI.
- **CI gate**: the tagged commit must have a successful `CI` run (`test` + `package`); the release workflow enforces this. The previous known success (`577e6a5`) predates the round-46 fixes, so a new green run is required before tagging.
- **Real-machine acceptance**: A1–A10 / A12–A13 are executed by the maintainer before release. Evidence is kept in `docs/product/acceptance/ACCEPTANCE-1.3.2.md` (docs are local-only and are not part of the tag).
- **Signing / update chain**: waived by the maintainer on 2026-10-22. Release artifacts are unsigned and upgrades are manual; `latest.json`/`.sig` are produced only when updater keys are configured. The unsigned path now requires the explicit `ALLOW_UNSIGNED_RELEASE=true` flag.
- **Backups**: `%PROGRAMDATA%\Remova\Backup` is created with a private DACL (SYSTEM/Administrators + owner). Only the new session is hardened strictly; historical sessions are hardened best-effort so an unreadable legacy session cannot block new backups. History files also receive the private DACL.
- **Round 46 final review (2026-10-22)**: 26 findings (2 P1 / 12 P2 / 12 P3) were addressed in the round-46 fix set. P1-01/P1-02 are closed by cross-hive isolation, server-scanned identity resolution and a cleanup allow-list. The report is `docs/reviews/2026-10-22/2026-10-22-第46轮-1.3.2发版前六路终审.md` (local).
- **Local gates at the fix set**: Rust 536 passed / 0 failed / 7 ignored; frontend 68 files / 510 tests; coverage 88.00/80.40/86.22/90.36; cargo fmt/clippy (`-D warnings`), lint, build, i18n (728 keys / 0 dead), jargon 0, scripts 16/16, commands 67/67, five-source versions and reviews-index (57 files) green.
- **Known design constraint**: the destructive transaction lock remains process-wide across backup/uninstall/delete so two cleanups cannot interleave. Cancellation is exposed in the UI and honoured at backup/restore/delete phase boundaries; a running vendor uninstaller is not killed mid-flight.
- **Round 44/45 history**: round-44 and round-45 findings were fixed in `f597c32`, `a34776e`, `577e6a5` and follow-up commits; round-45 P1-01 (global 1641 gate) is confirmed closed in round 46.

This file exists so a clean checkout/tag can see the release gates even though `docs/` is gitignored.
