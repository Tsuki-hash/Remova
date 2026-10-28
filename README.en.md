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
| **Deep-uninstall loop** | The confirm states the flow; a phase bar (detect → official uninstall → rescan → analyze → report); post-clean report and re-check |
| **Evidence scan** | Install dir, shortcuts, TEMP, registry, Run, services, tasks, **PATH, CLSID/shell, drivers, other-drive roots, WebView2 caches, home dot-directories, MuiCache** |
| **Plain language** | Why each leftover belongs (or why it is kept); Safe · Review · Keep buckets |
| **Orphan evidence** | Expandable evidence on orphan leftovers: no uninstall record / contains exes / config files / install root / recent modification |
| **Deep-uninstall extras** | Broken MSI records, orphan firewall rules, lock diagnosis (end a process under the app's install dir after confirmation), report totals and post-clean rescan/deep registry check |
| **User-data red line** | Documents / Downloads / sync-conflict folders are never auto-selected |
| **Safety Vault** | **Optional** backup before clean (unchecked by default); 7-day retention; open existing backups from report/history |
| **Verified clean** | Post-clean checklist (paths / registry / PATH) |
| **Orphans** | First-class page: leftovers with no install entry, grouped by origin |
| **System-item semantics** | Startup items = enabled/disabled; services split running state from start type (stopped ≠ disabled); scheduled tasks show last/next run |
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
4. **Report** deleted/kept/pending-reboot sizes + progress + Safety Vault note + verify checklist; rescan the app or run a read-only deep registry check  

### Clean & backup
- Dry-run first; real clean can opt into a safety backup (**unchecked by default**)  
- A system restore point is attempted before cleaning; turn it off under More  
- Shared runtimes (VC++ / .NET, …) unchecked by default  
- Backup root: `%PROGRAMDATA%\Remova\Backup\`

### More tools
- Orphans page, startup / services / scheduled tasks; broken MSI records and orphan firewall rules  
- Installer & updater caches, specialty caches, and the idle-app radar note “N more not shown” on their main lists (cache cleanup also lists the locations it could not read)  
- **Disk radar**: a read-only view of what takes the space; the overview pins each root's total ahead of size-ranked children (24 rows on the system drive, 12 elsewhere and when drilling down), unfinished directories show a `>=` floor with the entries checked, drill-down lists note “N more not shown”, and the system-drive overview states it covers the five known roots only  
- **C: drive space checkup**: overview (volume shape and junction ledger with dead links), cleanup & migration (action list with cleanable rows pinned and selectable, migration candidates handed off), and component store (read-only WinSxS DISM analysis)
- **Regenerable cache cleanup**: fixed layouts only (update downloads, temp dirs, crash dumps…), no backup by design; contents removed, directories kept, locked files reported honestly  
- **Directory migration (junction)**: move a folder to another drive — plan → copy (per-file diff) → cutover (auto-rollback) → finish (confirm to drop the backup); rollback available at every step  
- **Global CLI packages**: read-only npm / pnpm / yarn inventory; npm packages can be removed after confirmation (exported to the safety backup first)  
- **System traces & portable apps**: read-only report of orphaned ConsentStore entries and portable-app candidates (anything that did not pass the safety assessment is never landed in a cleanup)  
- History timeline, CSV export, install monitor  
- Dark / light / follow-system theme, Chinese / English

---

## Download

Get the latest build from **[Releases](https://github.com/Tsuki-hash/Remova/releases)**:

> In-app updates are not enabled (no updater public key is configured); always upgrade by downloading the new version from the Release page. If the Release has no Authenticode signature, verify the installer against `SHA256SUMS.txt` on the same page. When an updater public key is configured, the Release also provides `latest.json` and `.sig` (the updater manifest and its signature); without one they are not generated, and the MSI install is not wired into auto-update — download updates manually.
>
> Release modes: with no updater keys configured, the Release runs in unsigned/manual mode and publishes only the NSIS / MSI / portable artifacts plus `SHA256SUMS.txt`; no `latest.json` or `.sig` are produced. Updater signature assets are generated and verified only when all three updater settings are present.

| Artifact | Notes |
|---|---|
| `Remova_*_x64-setup.exe` | NSIS installer (recommended) |
| `Remova_*_x64_zh-CN.msi` | MSI installer (per-machine, Chinese-only wizard; non-admin needs UAC elevation) |
| `Remova_*_x64-portable.zip` | Portable zip — unzip and run |

Requires Windows 10 / 11 (x64) and the Microsoft Edge WebView2 Runtime (included in Windows 11 and most Windows 10 installs; the portable build does not bundle it). The NSIS installer downloads WebView2 when it is missing, so offline networks must install WebView2 first or use the MSI/portable build. Cleaning machine-wide apps, the machine PATH, services, firewall rules, or restoring into protected directories requires running Remova as administrator; without elevation these items fail individually and report the reason.

> **Portable trial**: copy `remova.exe` from `src-tauri/target/release/` on a dev machine. Registry scans work without install; machine-wide backup root is `%PROGRAMDATA%\Remova\Backup\`. Prefer NSIS / MSI for supported installs.

---

## Develop

### Requirements

| Tool | Version |
|---|---|
| Rust | ≥ 1.77.2 |
| Node.js | 22 |
| Windows | 10 / 11 |

### Commands

```powershell
# Frontend
npm ci
npm run build
npm run coverage

# Backend
cd src-tauri
cargo test --workspace
# Microbenchmarks for hot pure guards (optional, local only)
cargo bench

# Dev app
npx tauri dev

# Package
npx tauri build
```

See [CONTRIBUTING.md](./CONTRIBUTING.md) for conventions.

---

## Safety

- Real deletes require UI confirmation and pass `safety` checks.
- Analysis and cleanup use the latest server-side scan identity and paths. Same-name HKCU and HKLM entries are never merged; an elevated process refuses to analyze, uninstall or clean HKCU apps.
- Prefer **dry-run** before cleaning.
- Backups are best-effort and **not** a system backup substitute; the backup tree uses a private DACL (SYSTEM / Administrators / current user).
- A pending 1641 reboot blocks every cleanup and restore entry; an unreadable gate file is treated as pending (fail-closed).
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
2. Run `cargo test --workspace` and `npm run coverage`
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
