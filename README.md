# Remova

**Deep Uninstall for Windows** — Tauri 2 + React 19 + Rust.

Evidence-based association scan, safety preview, backup/restore, dry-run planning.

Docs: see [`docs/README.md`](./docs/README.md) for user guide, architecture, product roadmap.

## Features

- Installed app list (HKLM64 / HKLM32 / HKCU)
- Deep analyze: install dir, shortcuts, TEMP, registry, App Paths, Run, services, tasks
- Dry-run cleanup planning with item checkboxes
- Backup → cleanup with safety gates; optional official uninstaller
- Restore backup sessions; history + CSV
- Batch cleanup, dark/light theme, zh/en, disk usage

## Download

GitHub Releases: `Remova.exe`, `Remova_*_x64-setup.exe` (NSIS), `Remova_*_x64_en-US.msi`  
https://github.com/Tsuki-hash/Remova/releases

### Portable

NSIS/MSI installers are the supported path. For a semi-portable trial, copy `remova.exe` from `src-tauri/target/release/` — registry scans work without install; per-user backup root is `%PROGRAMDATA%\Remova\Backup\`. A dedicated portable zip is tracked as IMP-12.

## Develop

```powershell
npm.cmd install
npm.cmd run tauri dev
```

## Test / Build

```powershell
cd src-tauri && cargo test --lib
npm.cmd run build
npm.cmd run tauri build
```

## Safety

Real deletes require UI confirmation and pass `safety` checks. Backups: `%PROGRAMDATA%\Remova\Backup\`. Prefer dry-run first.

## License

MIT

## Disclaimer

Use at your own risk after reviewing the cleanup list. Backups are best-effort, not a system backup substitute.

## Legacy

Previous Python implementation: https://github.com/Tsuki-hash/Remova-python-legacy
