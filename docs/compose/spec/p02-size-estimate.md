---
feature: p02-size-estimate
status: delivered
updated: 2026-09-11
branch: main
commits: uncommitted-on-935f773 # commit P0-2+P0-3 when user confirms
---

# P0-2 Install Location Size Estimate

## Report

**What was built** — Apps missing registry `EstimatedSize` now get an on-disk size estimate from `install_location`. Backend walks the tree iteratively with a process-wide cancel flag (`dirsize.rs`); `estimate_dir_size_kb` runs on the blocking pool so large directories do not freeze the webview. Frontend queues missing paths with concurrency 2, caches results for the session, shows `~` + size for estimates, merges estimates into sort-by-size, and exposes 「停止估算」 while running.

**Verification** — `cargo test --lib` PASS (38 tests, including dirsize walk sum/cancel). `npm run build` (tsc + vite) PASS.

**Journey log**

1. Test initially failed because files were walked before flush/sync — fixed test to `sync_all` before walk.
2. First `estimate_dir_size_kb` was synchronous and would block UI on large trees — switched to `async` + `spawn_blocking`.
3. Independent reviewer subagent failed with UnknownError; completed a manual spec/diff review and fixed the blocking issue found in that pass.
4. Cancel mid-walk returns 0 from backend; frontend must not cache that 0 (guarded by `sizeCancelRef` after await).

## [S1] Problem

Many apps show `—` in the 占用 column because the uninstall registry has no `EstimatedSize`. Users cannot see disk impact or sort by real size, which undermines the “deep uninstall / reclaim space” promise (PRODUCT-GAPS P0-2).

## [S2] Design

**When**: After `list_installed_apps` loads, enqueue every app with `estimated_size_kb <= 0` and a non-empty `install_location` that exists on disk.

**Backend**

- New module `src-tauri/src/dirsize.rs`:
  - `walk_size_kb(root: &Path) -> i64` / `walk_size_kb_with(..., cancelled: &AtomicBool)`
  - Iterative directory walk, sum file lengths, check cancel, skip symlinks, return `0` on missing root or cancel.
- Commands in `lib.rs`:
  - `begin_size_estimate()` — clear cancel flag.
  - `estimate_dir_size_kb(path)` — **async**, runs walk on blocking pool; KB (ceil bytes/1024), `0` if missing.
  - `cancel_size_estimate()` — set process-wide `AtomicBool`; in-flight walks return `0`.

**Frontend**

- Map cache `install_location → size_kb` (session memory).
- Queue with concurrency `2`; 「停止估算」 cancels via backend flag and stops draining the queue.
- Display: registry size `> 0` as today; else estimate `> 0` → `~` + `formatSize`; else `—`.
- Sort by size uses registry || estimate.
- Estimated values are not written back into registry.

**Errors**: walk failures are non-fatal (leave `—`); do not block list interaction.

## [S3] Out of Scope

- P0-4 batch progress UI
- Persistent on-disk size cache across restarts
- Estimating apps without `install_location`
- Changing cleanup `size_bytes` planning (scan already measures items)

## Tasks

- [x] T1: `dirsize` walk + cancel flag + commands — acceptance: unit test sums a temp tree; cancel returns 0 early; `cargo test --lib` PASS (covers: S2)
- [x] T2: Frontend queue, cache, ~ display, stop control, sort merge — acceptance: UI shows `~` sizes for missing registry size; stop works; `npm run build` PASS (covers: S2; depends: T1)
- [x] T3: Update PRODUCT-GAPS P0-2 — acceptance: row checked with date (covers: S2)
