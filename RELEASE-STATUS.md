# Remova Release Status

## 1.3.2

- **Candidate**: latest commit on `main` (see CI badge / Actions for the exact SHA).
- **CI gate**: the tagged commit must have a successful `CI` run (`test` + `package`); the release workflow enforces this.
- **Real-machine acceptance**: A1–A10 / A12–A13 are executed by the maintainer before release. Evidence is kept in `docs/product/acceptance/ACCEPTANCE-1.3.2.md` (docs are local-only and are not part of the tag).
- **Signing / update chain**: waived by the maintainer on 2026-10-22. Release artifacts are unsigned and upgrades are manual; `latest.json`/`.sig` are produced only when updater keys are configured.
- **Backups**: `%PROGRAMDATA%\Remova\Backup` is created with a private DACL (SYSTEM/Administrators + owner).
- **Round 44 final review**: 25/25 findings fixed in `f597c32` (backend) and `a34776e` (frontend). Local gates: Rust 505 tests, frontend 66 files / 495 tests, coverage 87.44/77.58/85.17/89.70, lint/jargon/i18n/build green.
- **Known accepted risks (P2/P3)**: the round-44 report (`docs/reviews/2026-10-22/2026-10-22-第44轮-1.3.2发版前终审.md`, local) records the closed items and remaining external acceptance work.

This file exists so a clean checkout/tag can see the release gates even though `docs/` is gitignored.
