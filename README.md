# Remova

**Deep Uninstall for Windows** — Tauri 2 + React 19 + Rust.

Evidence-based association scan, safety preview, backup/restore, dry-run planning.

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
