# Contributing

## Environment

| Tool | Version |
|---|---|
| Rust | ≥ 1.95 |
| Node.js | 22.22.2+ within 22, 24.15.0+ within 24, or 26+ (CI uses Node 22) |
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
npm run check:reviews-index   # local-only: docs/ is gitignored, CI skips it
pwsh -NoProfile -File scripts/check-workflows-enabled.ps1
pwsh -NoProfile -File scripts/test-release-signing-policy.ps1
pwsh -NoProfile -File scripts/test-updater-signature.ps1

# Backend
cd src-tauri
cargo test --workspace
# Refusal path for a missing reboot-delete target: pure temp-dir fixture, no
# system state touched — runs as part of the default suite.
cargo test schedule_missing_path_no_panic
cargo fmt --check
cargo clippy --all-targets -- -D warnings
# Microbenchmarks for hot pure guards (dev-only; CI compiles them via
# clippy --all-targets but never runs them).
cargo bench
# CycloneDX SBOM from Cargo.lock + production npm deps (also attached to releases).
# Run from the repository root: the script resolves both lockfiles itself.
# From src-tauri use `node ../scripts/make-sbom.mjs`.
node scripts/make-sbom.mjs
node scripts/make-sbom.mjs --check-notices
# Regenerate and review when changing release dependencies.
node scripts/make-sbom.mjs --notices THIRD-PARTY-NOTICES.md

# Dependency audit: cargo audit (cargo-audit) and npm audit. CI runs both as
# release gates: `cargo audit` in src-tauri and `npm audit --omit=dev
# --audit-level=high`. Production npm dependencies must stay at zero
# vulnerabilities. Accepted Rust advisories live in `src-tauri/.cargo/audit.toml`
# with a reachability argument each — do not add a blanket ignore. Currently
# accepted (verify the rationale still holds before enabling updater assets):
#   RUSTSEC-2026-0367/0368/0369/0429 (zip 4.6.1, via tauri-plugin-updater 2.12):
#     upstream pins zip 4 (fix needs a later major); releases ship without
#     updater assets, so the extraction path is unreachable, and update packages
#     are minisign-verified before extraction. Remove these entries when the
#     plugin moves to a fixed zip and the lockfile is bumped.
cargo audit
npm audit --omit=dev --audit-level=high
# Targeted race fixture: requires Developer Mode or symlink privilege.
# CI and release run it explicitly; do not run every ignored system test.
cargo test copy_dir_refuses_file_swapped_to_symlink_after_enumeration -- --ignored
# Elevated only: isolated ProgramData key store, restricted same-user token ACLs,
# privileged backup/restore, restricted-token restore rejection, marker recovery
# and fail-closed key loss. No production key touched.
cargo test privileged_key_store_acl_and_missing_state_regression -- --ignored

# Ignored tests: what actually runs, and what is manual.
#
# The tree has 13 tests marked `#[ignore]`. CI runs five by name, including
# the two fixture tests above and three read-only native checks. Release runs
# the two fixture tests by name. The remaining eight cover optional capacity,
# performance and explicitly enabled real-system fixtures. See each test's
# ignore reason and the commands below before running one locally.
#
# | 测试 | 执行方式 | 它会动什么 |
# | --- | --- | --- |
# | `copy_dir_refuses_file_swapped_to_symlink_after_enumeration` | CI 自动 | 纯 temp dir；需要 symlink 特权或开发者模式 |
# | `privileged_key_store_acl_and_missing_state_regression` | CI 自动 | 测试 seals-root 下的隔离 ProgramData ACL 夹具；需要提权 token |
# | `startup_folder_approved_values_are_full_file_names` | CI 自动 | 只读；CI 检查实际运行了这个用例 |
# | `rename_value_refuses_existing_destination_without_changing_either` | 手动 | 往真实 HKCU Run 写/删带唯一前缀的临时值（RAII 清理）；需 `REMOVA_TEST_ALLOW_SYS_MUTATION=1` |
# | `read_reg_path_value_reads_real_user_environment` | CI 自动 | 只读，原生读 `HKCU\Environment` 的 Path |
# | `service_image_string_reads_real_image_paths` | CI 自动 | 只读，枚举 `HKLM64\SYSTEM\CurrentControlSet\Services` |
# | `restore_point_nonfatal` | 手动 | **创建真实系统还原点**（名称 `Remova test`）；需 `REMOVA_TEST_ALLOW_SYS_MUTATION=1` |
#
#   real registry / real machine state (stock machines may lack the fixture):
cargo test read_reg_path_value_reads_real_user_environment -- --ignored
cargo test service_image_string_reads_real_image_paths -- --ignored
cargo test startup_folder_approved_values_are_full_file_names -- --ignored
#   real-system side effects; opt in with REMOVA_TEST_ALLOW_SYS_MUTATION=1:
cargo test restore_point_nonfatal -- --ignored
# Writes temporary entries to the real HKCU Run key and may trigger security
# software startup prompts. Run explicitly on a disposable Windows test machine.
$env:REMOVA_TEST_ALLOW_SYS_MUTATION = "1"
try {
  cargo test rename_value_refuses_existing_destination_without_changing_either -- --ignored
} finally {
  Remove-Item Env:REMOVA_TEST_ALLOW_SYS_MUTATION
}

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
including path.json and only the selected registry import target. New sessions
also bind the file and directory manifest to the seal. The verifier retains
support for older RSEAL2 sessions. Those created without file digests provide
integrity for the path map and system snapshots only. The seal does not protect
against replay of an earlier valid state of the same session.
Tests with a seals-root override use a compile-time test key; production has no
key override, environment key, or generic signing IPC. Of the seven `#[ignore]`d
tests, `copy_dir_refuses_file_swapped_to_symlink_after_enumeration` and
`privileged_key_store_acl_and_missing_state_regression` run automatically in CI
and Release; the rest are manual (see the Commands block for what each one
mutates). A skipped test is not acceptance evidence — say so in the PR rather
than letting the default suite imply coverage it does not have.

## Build reproducibility (measured)

Two `cargo clean && cargo build --release` runs on the same machine, source tree and toolchain produce **byte-different** `remova.exe` binaries (measured 2026-10-25: three clean builds, three distinct SHA256 digests; the MSVC-linked image embeds nondeterministic data, typically the PDB GUID and linker records). Consequences:

- `SHA256SUMS.txt` on a Release page verifies **download integrity against that release's artifacts**; it does not claim cross-build reproducibility.
- If reproducible releases are ever needed, deterministic linking on MSVC or a toolchain switch must be evaluated and measured first — do not document reproducible builds until then.
