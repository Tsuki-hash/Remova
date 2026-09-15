## Remova 1.1.0

Deep uninstaller for Windows — explainable, reversible cleanup.

### Highlights

- **No more UI freezes** — heavy operations (analyze, batch cleanup, monitor) run off the main thread
- **Dry-run preview** — see planned/skipped items before deleting anything
- **Startup disable actually works** — uses Windows `StartupApproved` instead of renaming Run values
- **Safer cleanup** — unified path safety gates, 32-bit registry deletion fixed, uninstaller waited before residual delete
- **Batch tools** — optional official uninstaller, source filter, monitor-diff → cleanup list
- **Faster list** — virtualized table, fixed columns, no horizontal drag
- **Context menu** — "Remova Deep Uninstall" works and focuses the existing window

### Install

| Package | File |
|---|---|
| NSIS | `Remova_1.1.0_x64-setup.exe` |
| MSI | `Remova_1.1.0_x64_en-US.msi` |

Requires Windows 10/11 x64. Run as admin for system software cleanup.

### Known limitations

- No backup size cap yet
- `schtasks` parsing may fail on rare system locales
