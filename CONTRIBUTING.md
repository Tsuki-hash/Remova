# Contributing

## Environment

| Tool | Version |
|---|---|
| Rust | ≥ 1.77 |
| Node.js | 22 (CI uses 22) |
| Windows | 10/11 (backend is Windows-only) |

## Commands

```powershell
# Frontend
npm ci
npm run build      # tsc + vite
npm test           # vitest

# Backend
cd src-tauri
cargo test --lib
cargo fmt --check
cargo clippy --all-targets -- -D warnings

# Full app
npx tauri dev
npx tauri build
npm run smoke:dist      # after npm run build
npm run smoke:portable  # after package:portable
```

## Conventions

- Conventional Commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`).
- Keep zh/en i18n keys in lockstep (`src/i18n.ts` has a compile-time key-parity check).
- New Tauri commands that do IO must be `async` + `spawn_blocking`.
- Safety gates live in `src-tauri/src/safety.rs` — do not duplicate `is_safe_fs`.
- Prefer absolute `System32` paths for system tools (`regops::sys_tool`).

## Pull requests

1. Run `cargo test --lib` and `npm test` before opening.
2. Keep diffs focused; no drive-by reformatting.
3. Update `CHANGELOG.md` for user-visible changes.
