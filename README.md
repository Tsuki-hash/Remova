# Remova-next

Tauri 2 + React 19 + Rust rewrite of Remova (Phase 0: read-only installed app list).

## Requirements

- Node.js 20+
- Rust 1.77+ (stable MSVC toolchain on Windows)
- Tauri CLI 2 (`npm i` installs `@tauri-apps/cli`)

## Develop

```powershell
cd Remova-next
npm install
npm run tauri dev
```

## Test

```powershell
cd src-tauri
cargo test
```

## Build

```powershell
npm run tauri build
```

## Layout

- `src/` — React 19 + TypeScript UI
- `src-tauri/` — Rust core (`apps` registry scan, `safety` guards)
- `docs/compose/spec/` — feature specs
