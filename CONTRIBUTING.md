# Contributing

## Environment

| Tool | Version |
|---|---|
| Rust | ≥ 1.77 |
| Node.js | 22 (CI uses 22) |
| PowerShell | **7 (`pwsh`)** — `check-versions`, `check-commands`, `smoke:*`, `package-portable` are `.ps1` scripts and Windows PowerShell 5.1's `powershell.exe` is not a substitute |
| Windows | 10/11 (backend is Windows-only) |

## Commands

```powershell
# Frontend
npm ci
npm run build      # tsc + vite
npm run coverage   # vitest + coverage gate
npm run lint
npm run test:scripts # CLI regression tests
npm run check:i18n   # read-only unused-key gate; never prunes files
npm run check-versions
npm run check-commands

# Backend
cd src-tauri
cargo test --workspace
cargo fmt --check
cargo clippy --all-targets -- -D warnings
# Targeted race fixture: requires Developer Mode or symlink privilege.
# CI and release run it explicitly; do not run every ignored system test.
cargo test copy_dir_refuses_file_swapped_to_symlink_after_enumeration -- --ignored
# Elevated only: isolated ProgramData key store, restricted same-user token ACLs,
# trusted staging, marker recovery and fail-closed key loss. No production key touched.
cargo test privileged_key_store_acl_and_missing_state_regression -- --ignored

# Full app
npx tauri dev
npx tauri build
npm run smoke:dist      # after a build (npm run build or tauri build)
npm run smoke:app
npm run smoke:launch
npm run smoke:portable  # after package:portable
```

## Conventions

- Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`).
- Keep zh/en i18n keys in lockstep (`src/i18n/index.ts` has a compile-time key-parity check).
- `node scripts/find-dead-i18n.mjs` reports unused keys; `--check` fails on them without
  writing files. Use `--prune` explicitly when removing keys, then review the diff.
  This is a conservative text-reference check, not a semantic TypeScript analysis.
- New Tauri commands that do IO must be `async` + `spawn_blocking`.
- Safety gates live in `src-tauri/src/safety.rs` — do not duplicate `is_safe_fs`. Anything about to
  *delete* must use `is_safe_fs_for_delete` (delete-grade gate) via `policy::gate_cleanup_item`.
- Prefer absolute `System32` paths for system tools (`regops::sys_tool`).

## Pull requests

1. Run `cargo test --workspace` and `npm run coverage` before opening.
2. Keep diffs focused; no drive-by reformatting.
3. Update `CHANGELOG.md` for user-visible changes.

## Backup seal compatibility

RSEAL2 backup/restore requires elevation. Its CNG HMAC key lives in the OS Known
Folder ProgramData/RemovaSealKeys, protected for SYSTEM and Administrators only.
A missing key in an existing store is an error, never automatic regeneration.
RSEAL1 sessions remain available for inspection/manual export; automatic restore
rejects them. Backup is one-shot per session and signs private staging snapshots,
including path.json and only the selected registry import target. Ordinary file
contents and rollback to earlier valid states are not covered by this seal.
Tests with a seals-root override use a compile-time test key; production has no
key override, environment key, or generic signing IPC. Ignored privilege tests
must run explicitly in CI/release; a skipped test is not acceptance evidence.
