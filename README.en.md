<div align="center">

<img src="src-tauri/icons/icon.png" alt="Remova" width="96" />

# Remova

**Deep uninstall for Windows — clean removal, explained**

Find it. Explain it. Remove it. Restore it.

[English](./README.en.md) · [简体中文](./README.md)

[![Release](https://img.shields.io/github/v/release/Tsuki-hash/Remova?style=flat-square&label=release)](https://github.com/Tsuki-hash/Remova/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Tsuki-hash/Remova/total?style=flat-square)](https://github.com/Tsuki-hash/Remova/releases)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](./LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6?style=flat-square)](https://github.com/Tsuki-hash/Remova/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/Tsuki-hash/Remova/ci.yml?branch=main&style=flat-square&label=CI)](https://github.com/Tsuki-hash/Remova/actions)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)
[![React](https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=black)](https://react.dev)
[![Rust](https://img.shields.io/badge/Rust-1.77%2B-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)

</div>

---

## Why Remova

Stock uninstallers leave registry keys, services, PATH entries, CLSIDs, and cross-drive caches. Generic “cleaners” often delete *your* files by mistake.

Remova is built around **Find it. Explain it. Remove it. Restore it.**

| Capability | What you get |
|---|---|
| **Decision UI** | Detail drawer before uninstall: size, links, health, official / deep / force modes |
| **Evidence scan** | Install dir, shortcuts, TEMP, registry, Run, services, tasks, **PATH, CLSID/shell, drivers, other-drive roots, WebView2 caches** |
| **Plain language** | Why each leftover belongs (or why it is kept); Safe · Review · Keep buckets |
| **User-data red line** | Documents / Downloads / sync-conflict folders are never auto-selected |
| **Safety Vault** | **Optional** backup before clean (unchecked by default); 7-day retention; open existing backups from report/history |
| **Verified clean** | Post-clean checklist (paths / registry / PATH) |
| **Orphans** | First-class page: leftovers with no install entry, grouped by origin |
| AI assist (optional) | Explain leftovers & risk, NL plans — advisory only |

> Brand: calm, professional, reversible — not a “PC manager”.

---

## Features

### Installed apps
- Sources: HKLM64 / HKLM32 / HKCU / Microsoft Store (MSIX), deduplicated
- Decision chips: large / recent / recommended / no uninstaller / store vs desktop
- Search, sort, virtualized list; drop exe or install folder to analyze

### Deep uninstall flow
1. **Identify** install type and associations  
2. **Official uninstaller** first when available  
3. **Scan leftovers** with live Safe / Review / Keep overview  
4. **Report** counts + progress + Safety Vault note + verify checklist  

### Clean & backup
- Dry-run first; real clean can opt into a safety backup (**unchecked by default**)  
- Shared runtimes (VC++ / .NET, …) unchecked by default  
- Backup root: `%PROGRAMDATA%\Remova\Backup\`

### More tools
- Orphans page, startup / services / scheduled tasks  
- History timeline, CSV export, install monitor  
- Dark / light theme, Chinese / English

---

## Download

Get the latest build from **[Releases](https://github.com/Tsuki-hash/Remova/releases)**:

| Artifact | Notes |
|---|---|
| `Remova_*_x64-setup.exe` | NSIS installer (recommended) |
| `Remova_*_x64_zh-CN.msi` | MSI installer |
| `Remova_*_x64-portable.zip` | Portable zip — unzip and run |

Requires Windows 10 / 11 (x64).

> **Portable trial**: copy `remova.exe` from `src-tauri/target/release/` on a dev machine. Registry scans work without install; per-user backup root is `%PROGRAMDATA%\Remova\Backup\`. Prefer NSIS / MSI for supported installs.

---

## Develop

### Requirements

| Tool | Version |
|---|---|
| Rust | ≥ 1.77 |
| Node.js | 22 |
| Windows | 10 / 11 |

### Commands

```powershell
# Frontend
npm ci
npm run build
npm test

# Backend
cd src-tauri
cargo test --workspace

# Dev app
npx tauri dev

# Package
npx tauri build
```

See [CONTRIBUTING.md](./CONTRIBUTING.md) for conventions.

---

## Safety

- Real deletes require UI confirmation and pass `safety` checks.
- Prefer **dry-run** before cleaning.
- Backups are best-effort and **not** a system backup substitute.
- Services and scheduled tasks are treated as high risk and unchecked by default.

---

## Stack

```text
UI      React 19 + Vite + TypeScript
Shell   Tauri 2
Core    Rust (registry / FS / services / tasks)
Tests   Vitest + cargo test
Ship    NSIS / MSI
```

---

## Repository layout (public)

```text
Remova/
├── src/                 # React frontend
├── src-tauri/           # Rust backend & packaging
├── scripts/             # Build helpers
├── .github/workflows/   # CI / Release
├── CHANGELOG.md
├── CONTRIBUTING.md
├── README.md            # 中文
└── README.en.md         # English (this file)
```

---

## Contributing

Issues and PRs are welcome:

1. Fork and open a feature branch
2. Run `cargo test --workspace` and `npm test`
3. Use Conventional Commits (`feat:`, `fix:`, `docs:`, …)
4. Update `CHANGELOG.md` for user-visible changes

Details: [CONTRIBUTING.md](./CONTRIBUTING.md).

---

## License

[MIT License](./LICENSE)

---

## Disclaimer

Review the cleanup list and use at your own risk. Backups are best-effort and are not a complete system backup or data-recovery guarantee.

---

<div align="center">

If Remova helps you, a ⭐ Star is appreciated

[![Star](https://img.shields.io/github/stars/Tsuki-hash/Remova?style=social)](https://github.com/Tsuki-hash/Remova)

</div>
