---
feature: p04-batch-progress
status: delivered
updated: 2026-09-11
branch: main
commits: uncommitted-on-5bc54b3
---

# P0-4 Batch Progress / Cancel / Failure Summary

## Report

**What was built** — Batch cleanup now tracks per-app outcomes. Progress bar shows `index/total` and the current app name; the batch button becomes 「取消批量」 which stops after the current app. Each app is isolated in try/catch: analyze/cleanup errors are recorded and the queue continues. After finish, a summary card lists ok/failed/skipped with 「重试失败项」 that re-selects only failed apps.

**Verification** — `npm run build` (tsc + vite) PASS. Interactive multi-app batch was not run headlessly (needs Tauri window).

**Journey log**

1. Cancel is cooperative between apps only — in-flight `run_full_cleanup` still completes (documented out of scope).
2. Partial cleanup (`deleted>0` but `failed>0`) is treated as ok with detail so users see mixed results without retrying a mostly-done app.

## [S1] Problem

Batch cleanup (`batchCleanup`) only shows `[i/n]` in a notice, aborts the whole queue on the first error, and offers no mid-run cancel or per-app outcome list (PRODUCT-GAPS P0-4).

## [S2] Design

**Frontend-only** (backend already analyzes/cleans one app per invoke).

- State: `{ running, index, total, name, cancelled, results[] }` where `results` items are `{ key, name, status: ok|failed|skipped, detail }`.
- Progress UI: visible while running — bar + `index/total` + current app name; button becomes 「取消批量」.
- Cancel: sets flag; loop checks **after each app**.
- Per-app try/catch: failure records `failed` and continues; no confirmed items → `skipped`; success → `ok`. `failed>0 && deleted===0` → failed; otherwise ok with detail.
- Summary card after finish: counts + list; 「重试失败项」 re-queues failed apps only.
- `multi` retains failures for retry; successful keys are cleared from selection.
- i18n zh/en for cancel, summary, retry.

## [S3] Out of Scope

- Abort inside `run_full_cleanup` / uninstaller
- Parallel multi-app cleanup
- Persistent batch history (cleanup history already exists)

## Tasks

- [x] T1: batch state machine + per-app error isolation + cancel flag — acceptance: code review (covers: S2)
- [x] T2: progress bar, cancel control, summary card, retry failed — acceptance: `npm run build` PASS (covers: S2; depends: T1)
- [x] T3: PRODUCT-GAPS P0-4 check — acceptance: row checked (covers: S2)
