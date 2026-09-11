# Remova-next

Tauri 2 + React 19 + Rust rewrite of Remova.

## Features

- Installed app list (HKLM64 / HKLM32 / HKCU)
- Deep analyze with evidence scores (read-only)
- Dry-run cleanup planning
- Backup → cleanup with safety gates
- Optional official uninstaller
- Restore latest backup
- Local history

## Requirements

- Windows 10/11
- Node.js 20+
- Rust stable MSVC toolchain

## Develop

```powershell
cd Remova-next
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

Real deletes require UI confirmation and pass `safety` checks. Backups: `%PROGRAMDATA%\Remova\Backup\`.
