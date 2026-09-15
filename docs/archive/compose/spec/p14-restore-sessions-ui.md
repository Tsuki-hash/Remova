---
feature: p14-restore-sessions-ui
status: delivered
updated: 2026-09-11
branch: main
commits: uncommitted-on-f82a638
---

# P1-4 Restore Session Picker UI

## Report

**What was built** — 「还原最近备份」 now opens a session list card (radio select + scroll), then 「确认还原」 calls `restore_session_by_name`. Results render in the card instead of `alert`/`prompt`. Empty state shows 暂无备份会话.

**Verification** — `npm run build` PASS. Backend commands unchanged (`list_restore_sessions`, `restore_session_by_name`).

## [S1] Problem

Restore used `window.prompt` with a truncated name list — hard to use and easy to typo (PRODUCT-GAPS P1-4 / PARITY 🟡).

## [S2] Design

Frontend-only: panel state (`showRestore`, sessions, pick, busy, msgs); confirm still uses `restoreConfirm`; messages capped at 20 lines.

## [S3] Out of Scope

- Session metadata (time/size) beyond folder name
- Partial item restore

## Tasks

- [x] T1: session list UI + restore invoke + result panel — acceptance: build PASS
- [x] T2: PRODUCT-GAPS P1-4 checked
