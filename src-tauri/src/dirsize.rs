//! Directory size walk for apps missing registry EstimatedSize.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

static CANCELLED: AtomicBool = AtomicBool::new(false);
/// Monotonic estimate-batch id: begin/cancel bump it so in-flight walks from older batches become stale.
static BATCH: AtomicU64 = AtomicU64::new(0);

/// Request cancellation of in-flight / queued size walks (legacy global flag).
pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
    BATCH.fetch_add(1, Ordering::SeqCst);
}

/// Clear cancel flag before starting a new estimate batch/job.
pub fn clear_cancel() {
    CANCELLED.store(false, Ordering::SeqCst);
    BATCH.fetch_add(1, Ordering::SeqCst);
}

pub fn current_batch() -> u64 {
    BATCH.load(Ordering::SeqCst)
}

pub fn batch_stale(gen: u64) -> bool {
    BATCH.load(Ordering::SeqCst) != gen
}

/// Limits for scan-time leftover size attachment (never block analyze).
pub(crate) const MAX_WALK_ENTRIES: u64 = 5_000;
const MAX_WALK_DEPTH: u32 = 8;

/// Sum file sizes under `root` in KB (ceil). Returns 0 if missing, cancelled, or empty.
pub fn walk_size_kb(root: &Path) -> i64 {
    walk_size_kb_with(root, &CANCELLED)
}

/// Like [`walk_size_kb`] but also reports an incomplete walk (budget or IO errors).
/// Honors the global cancel flag and the calling scan's cancellation.
pub fn walk_size_kb_capped(root: &Path) -> (i64, bool) {
    let _slot = acquire_walk_slot();
    walk_size_kb_with_capped(root, &CANCELLED)
}

/// Sum file sizes under `root` in KB (ceil). Returns 0 if missing, cancelled, or empty.
/// Second element is `true` for budget or IO truncation: size is then a lower bound.
pub fn walk_size_kb_with_capped(root: &Path, cancelled: &AtomicBool) -> (i64, bool) {
    let _slot = acquire_walk_slot();
    match root.try_exists() {
        Ok(false) => return (0, false),
        Err(_) => return (0, true),
        Ok(true) => {}
    }
    let (bytes, capped) = walk_size_bytes_with(root, cancelled);
    if bytes == 0 {
        (0, capped)
    } else {
        (bytes.div_ceil(1024) as i64, capped)
    }
}

pub fn walk_size_kb_with(root: &Path, cancelled: &AtomicBool) -> i64 {
    walk_size_kb_with_capped(root, cancelled).0
}

/// Bounded walk for leftover items: depth + entry caps, no global cancel flag.
/// Returns None when a cap or IO error leaves an incomplete size.
/// Entries a single size-estimation pass may examine across all items.
///
/// The per-directory budget is 5,000 entries, so with no overall budget the cost
/// of `fill_item_sizes` grew linearly with the number of items — up to 200 items
/// on the tool-cache scan, and unbounded on `analyze_associations`. This makes
/// the worst case a constant instead.
pub const SIZE_WALK_ENTRY_BUDGET: u64 = 20_000;

// Thread-local so concurrent blocking scans each get their own budget.
thread_local! {
    static WALK_BUDGET: std::cell::Cell<u64> = const { std::cell::Cell::new(SIZE_WALK_ENTRY_BUDGET) };
    /// A pass-wide budget shared by every worker of one parallel size pass.
    /// `None` in the ordinary serial case, where each thread has its own.
    static SHARED_BUDGET: std::cell::RefCell<Option<SharedWalkBudget>> =
        const { std::cell::RefCell::new(None) };
}

/// Charge `n` entries against the budget in force on this thread. `false` once
/// exhausted.
pub fn charge_walk_budget(n: u64) -> bool {
    // PERF-E: borrow the pool instead of cloning the Arc on every entry.
    if let Some(charged) = SHARED_BUDGET.with(|s| s.borrow().as_ref().map(|pool| pool.charge(n))) {
        return charged;
    }
    WALK_BUDGET.with(|b| {
        let left = b.get();
        if left < n {
            b.set(0);
            false
        } else {
            b.set(left - n);
            true
        }
    })
}

/// Reset this thread's budget for a fresh scan pass.
pub fn reset_walk_budget() {
    WALK_BUDGET.with(|b| b.set(SIZE_WALK_ENTRY_BUDGET));
}

/// This thread's remaining walk budget.
///
/// A parallel size pass seeds its shared pool with what the calling thread had
/// left, so N workers cannot turn one bounded pass into N of them.
pub fn remaining_walk_budget() -> u64 {
    if let Some(left) = SHARED_BUDGET.with(|s| s.borrow().as_ref().map(|pool| pool.remaining())) {
        return left;
    }
    WALK_BUDGET.with(|b| b.get())
}

/// One entry budget shared by every worker of a parallel size pass.
///
/// Workers draw from the same pool rather than each getting a slice, so the bound
/// stays one bound however many workers there are — and a worker that finishes
/// early leaves its unused quota for the others instead of wasting it. The
/// thread-local budget is untouched for the serial case, so concurrent scans
/// keep their own.
#[derive(Clone)]
pub struct SharedWalkBudget(Arc<AtomicU64>);

impl SharedWalkBudget {
    pub fn seeded(entries: u64) -> Self {
        Self(Arc::new(AtomicU64::new(entries)))
    }

    /// Charge `n` entries. `false` once the pool is exhausted.
    pub fn charge(&self, n: u64) -> bool {
        let mut left = self.0.load(Ordering::SeqCst);
        loop {
            if left < n {
                return false;
            }
            match self
                .0
                .compare_exchange_weak(left, left - n, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return true,
                Err(actual) => left = actual,
            }
        }
    }

    pub fn remaining(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    /// Make this pool the budget in force on the calling thread. The returned
    /// guard restores the previous one when the worker is done.
    pub fn install(&self) -> SharedWalkBudgetGuard {
        let previous = SHARED_BUDGET.with(|s| s.borrow_mut().replace(self.clone()));
        SharedWalkBudgetGuard(previous)
    }
}

/// Restores the budget that was in force before a worker installed a pool.
pub struct SharedWalkBudgetGuard(Option<SharedWalkBudget>);

impl Drop for SharedWalkBudgetGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        SHARED_BUDGET.with(|s| *s.borrow_mut() = previous);
    }
}

/// Global cap on threads inside one size walk.
///
/// Concurrent scans shared no bound at all, so a machine with idle cores did
/// that many directory walks at once and the interactive size estimate queued
/// behind every one of them.
pub(crate) const MAX_CONCURRENT_WALKERS: usize = 3;

/// Slots held back for a foreground walk — one started by a direct user request
/// rather than from inside a batch scan — so it always finds a free slot instead
/// of waiting for the scans to finish.
const FOREGROUND_RESERVED_SLOTS: usize = 1;

static WALKERS: AtomicUsize = AtomicUsize::new(0);
static FOREGROUND_WAITERS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// True while this thread already holds a walk slot, so a worker walking item
    /// after item takes exactly one slot for the whole batch.
    static HOLDS_WALK_SLOT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// RAII size-walk concurrency slot. Dropping it frees the slot.
pub struct WalkSlot {
    foreground: bool,
}

impl Drop for WalkSlot {
    fn drop(&mut self) {
        WALKERS.fetch_sub(1, Ordering::SeqCst);
        HOLDS_WALK_SLOT.with(|h| h.set(false));
        if self.foreground {
            FOREGROUND_WAITERS.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Take one of the global size-walk slots, yielding until one is free.
///
/// `None` means this thread already holds a slot — a nested walk — so there is
/// nothing to release. A direct size request counts as foreground; a scan pass
/// (a thread inside `scan_task::run_scoped` or a worker it spawned) counts as
/// background. Background walks stop taking the last slot while a foreground
/// walk is waiting, which is what makes a direct size request never queue
/// behind concurrent scans without starving them: the restriction lifts the
/// moment no foreground walk is waiting.
pub fn acquire_walk_slot() -> Option<WalkSlot> {
    // PERF-B: the foreground/background split used to read the thread-local
    // `scan_task::has_task()`, but a worker spawned inside a scan does not
    // inherit that thread-local, so a scan's own walkers were all judged
    // foreground — the scan filled the reserved slots itself and squeezed real
    // background walks to fewer slots. The distinction is now explicit.
    acquire_walk_slot_classified(!crate::scan_task::has_task(), None)
}

/// Like [`acquire_walk_slot`], but the walk is unconditionally background —
/// for walkers that a scan pass spawned directly and can classify themselves.
pub fn acquire_walk_slot_background() -> Option<WalkSlot> {
    acquire_walk_slot_classified(false, None)
}

/// Cancellable background slot acquisition for workers without thread-local scan
/// context. Returns `None` when the scan was cancelled while waiting.
pub fn acquire_walk_slot_background_with_cancel(
    cancel: &crate::scan_task::CancelWatch,
) -> Option<WalkSlot> {
    acquire_walk_slot_classified(false, Some(cancel))
}

fn acquire_walk_slot_classified(
    foreground: bool,
    cancel: Option<&crate::scan_task::CancelWatch>,
) -> Option<WalkSlot> {
    // A thread that already holds a slot must not take a second one: a worker
    // walks item after item under ONE slot, and its per-row walks are nested.
    // This check lives here rather than in one entry point so the background
    // path cannot bypass it — doing so deadlocked the parallel sizing pass
    // (every worker held one slot while its rows waited for a fourth).
    if HOLDS_WALK_SLOT.with(|h| h.get()) {
        return None;
    }
    if foreground {
        FOREGROUND_WAITERS.fetch_add(1, Ordering::SeqCst);
    }
    let mut backoff = std::time::Duration::from_millis(1);
    loop {
        if cancel.is_some_and(|watch| watch.cancelled()) || crate::scan_task::cancelled() {
            if foreground {
                FOREGROUND_WAITERS.fetch_sub(1, Ordering::SeqCst);
            }
            return None;
        }
        let limit = walk_limit(foreground, FOREGROUND_WAITERS.load(Ordering::SeqCst));
        let running = WALKERS.load(Ordering::SeqCst);
        if running < limit
            && WALKERS
                .compare_exchange(running, running + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            HOLDS_WALK_SLOT.with(|h| h.set(true));
            return Some(WalkSlot { foreground });
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(std::time::Duration::from_millis(16));
    }
}

/// How many walks may run right now.
///
/// A background walk never takes the last slot while a foreground walk is
/// waiting, so there is always one free for it. The reservation is smaller than
/// the cap, which is what keeps that from starving background walks: the moment
/// no foreground walk is waiting, they get every slot back.
fn walk_limit(foreground: bool, foreground_waiters: usize) -> usize {
    if foreground || foreground_waiters == 0 {
        MAX_CONCURRENT_WALKERS
    } else {
        MAX_CONCURRENT_WALKERS - FOREGROUND_RESERVED_SLOTS
    }
}

pub fn walk_size_kb_limited(root: &Path) -> Option<u64> {
    match walk_size_kb_with_quota(root, u64::MAX) {
        WalkSize::Exact(kb) => Some(kb),
        WalkSize::Floor(_) | WalkSize::Unknown => None,
    }
}

/// Outcome of one bounded size walk inside a scan pass.
///
/// `Exact` is the true total. `Floor` is a partial sum that stayed within every
/// bound it hit — a valid lower bound, safe to publish as `>= N` (the
/// size_capped contract: a truncated value must never be shown as exact).
/// `Unknown` is no
/// trustworthy answer at all: the path vanished, an entry could not be read, or
/// the scan was cancelled mid-walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkSize {
    Exact(u64),
    Floor(u64),
    Unknown,
}

/// Per-row entry sub-quota for one walk (PERF-A). The walk stops early — with
/// a `Floor`, never a fake exact — once it has charged this many entries.
pub struct WalkQuota {
    limit: u64,
    used: std::cell::Cell<u64>,
}

impl WalkQuota {
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            used: std::cell::Cell::new(0),
        }
    }

    /// Charge one entry. `false` once this walk's sub-quota is spent.
    fn charge(&self) -> bool {
        let used = self.used.get() + 1;
        if used > self.limit {
            false
        } else {
            self.used.set(used);
            true
        }
    }
}

/// One row's size under a per-row entry sub-quota (PERF-A).
///
/// `Exact` when the walk finished inside every bound; a `Floor` partial sum
/// when it stopped on a bound (this walk's sub-quota, the pass-wide pool, or
/// the per-walk entry cap — all leave a valid lower bound); `Unknown` on an
/// I/O error, a vanished path, or a cancelled scan.
pub fn walk_size_kb_with_quota(root: &Path, quota: u64) -> WalkSize {
    // Same classification as `acquire_walk_slot`: a thread inside a scan task
    // is background, everything else (a direct user request) is foreground.
    walk_size_kb_with_quota_bg(root, quota, crate::scan_task::has_task())
}

/// [`walk_size_kb_with_quota`] with an explicit slot classification (PERF-B).
/// `background = true` stops the walk from occupying the foreground reserve —
/// scan-pass walkers must report themselves as background so the reserve stays
/// free for direct size requests.
pub fn walk_size_kb_with_quota_bg(root: &Path, quota: u64, background: bool) -> WalkSize {
    walk_size_kb_with_quota_bg_cancel(root, quota, background, None)
}

/// Quota-aware scan walk which observes a worker-propagated cancellation watch
/// inside the directory-entry loop (thread-local scan state is not inherited).
pub fn walk_size_kb_with_quota_bg_cancel(
    root: &Path,
    quota: u64,
    background: bool,
    cancel: Option<&crate::scan_task::CancelWatch>,
) -> WalkSize {
    let _slot = if background {
        match cancel {
            Some(watch) => acquire_walk_slot_background_with_cancel(watch),
            None => acquire_walk_slot_background(),
        }
    } else {
        acquire_walk_slot()
    };
    if cancel.is_some_and(crate::scan_task::CancelWatch::cancelled) || crate::scan_task::cancelled()
    {
        return WalkSize::Unknown;
    }
    match root.metadata() {
        Ok(meta) if !meta.is_dir() => {
            if meta.is_file() {
                return WalkSize::Exact(meta.len().div_ceil(1024));
            }
            return WalkSize::Unknown;
        }
        Ok(_) => {}
        Err(_) => return WalkSize::Unknown,
    }
    let mut entries_seen = 0u64;
    let walk_quota = WalkQuota::new(quota);
    match walk_bytes_limited_quota_with_cancel(
        root,
        0,
        &mut entries_seen,
        &walk_quota,
        &|p: &Path| std::fs::read_dir(p),
        &std::fs::DirEntry::metadata,
        cancel,
    ) {
        WalkSize::Exact(bytes) => WalkSize::Exact(bytes.div_ceil(1024)),
        WalkSize::Floor(bytes) => WalkSize::Floor(bytes.div_ceil(1024)),
        WalkSize::Unknown => WalkSize::Unknown,
    }
}

/// Entry ceiling for one [`walk_size_kb_unbudgeted`] call. Deliberately its own
/// `AtomicU64` per walk instead of `charge_walk_budget`: the thread-local /
/// shared budgets belong to scan passes (reset in `fill_item_sizes`), and a
/// blocking-pool thread reused from a spent scan used to make every later walk
/// report `None` — the Safety Vault's all-sessions-show-0-KB defect.
const UNBUDGETED_DIR_CEILING: u64 = 20_000;

/// Bounded walk with its own local ceiling — no global cancel flag, no
/// thread-local budget, no shared pool.
///
/// For callers outside a scan pass (`restore::dir_size_kb`): thread reuse must
/// not change the answer, and `scan_task::cancelled()` is a no-op off a scan
/// thread, so the scan-time machinery can only make results worse here.
pub fn walk_size_kb_unbudgeted(root: &Path) -> Option<u64> {
    let _slot = acquire_walk_slot();
    let meta = root.metadata().ok()?;
    if !meta.is_dir() {
        if meta.is_file() {
            return Some(meta.len().div_ceil(1024));
        }
        return None;
    }
    let seen = std::sync::atomic::AtomicU64::new(0);
    let bytes = walk_unbudgeted_with(root, 0, &seen)?;
    Some(bytes.div_ceil(1024))
}

fn walk_unbudgeted_with(
    dir: &Path,
    depth: u32,
    seen: &std::sync::atomic::AtomicU64,
) -> Option<u64> {
    if depth > MAX_WALK_DEPTH
        || seen.load(std::sync::atomic::Ordering::Relaxed) >= UNBUDGETED_DIR_CEILING
    {
        return None;
    }
    let mut total: u64 = 0;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        if seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1 > UNBUDGETED_DIR_CEILING {
            return None;
        }
        let meta = entry.metadata().ok()?;
        if is_reparse_metadata(&meta) {
            continue;
        }
        if meta.is_dir() {
            total = total.saturating_add(walk_unbudgeted_with(&entry.path(), depth + 1, seen)?);
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    Some(total)
}

/// Quota-aware walk for scan passes (PERF-A): `Exact` when it finished, a
/// `Floor` when it stopped on a bound it can honestly vouch for, `Unknown` when
/// an entry could not be read or the scan was cancelled.
///
/// `pub(crate)` so the error-path tests with injected readers and the scanner's
/// fixture tests drive the same core `fill_item_sizes` uses.
#[cfg(test)]
pub(crate) fn walk_bytes_limited_quota_with<F, I>(
    dir: &Path,
    depth: u32,
    entries_seen: &mut u64,
    quota: &WalkQuota,
    read: &F,
    metadata: &impl Fn(&std::fs::DirEntry) -> std::io::Result<std::fs::Metadata>,
) -> WalkSize
where
    F: Fn(&Path) -> std::io::Result<I>,
    I: Iterator<Item = std::io::Result<std::fs::DirEntry>>,
{
    walk_bytes_limited_quota_with_cancel(dir, depth, entries_seen, quota, read, metadata, None)
}

fn walk_bytes_limited_quota_with_cancel<F, I>(
    dir: &Path,
    depth: u32,
    entries_seen: &mut u64,
    quota: &WalkQuota,
    read: &F,
    metadata: &impl Fn(&std::fs::DirEntry) -> std::io::Result<std::fs::Metadata>,
    cancel: Option<&crate::scan_task::CancelWatch>,
) -> WalkSize
where
    F: Fn(&Path) -> std::io::Result<I>,
    I: Iterator<Item = std::io::Result<std::fs::DirEntry>>,
{
    if depth > MAX_WALK_DEPTH
        || crate::scan_task::cancelled()
        || cancel.is_some_and(crate::scan_task::CancelWatch::cancelled)
    {
        return WalkSize::Unknown;
    }
    let mut total: u64 = 0;
    let mut floor = false;
    let entries = match read(dir) {
        Ok(entries) => entries,
        Err(_) => return WalkSize::Unknown,
    };
    for entry in entries {
        if crate::scan_task::cancelled()
            || cancel.is_some_and(crate::scan_task::CancelWatch::cancelled)
        {
            return WalkSize::Unknown;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => return WalkSize::Unknown,
        };
        *entries_seen += 1;
        if *entries_seen >= MAX_WALK_ENTRIES {
            return floor_walk(total);
        }
        // The pass-wide pool is shared across every item in one pass, so a scan
        // with many items cannot multiply the per-directory cost. Each row also
        // has its own sub-quota, so the pool cannot be eaten by the first rows.
        let charged_quota = quota.charge();
        if !charged_quota || !charge_walk_budget(1) {
            return floor_walk(total);
        }
        let meta = match metadata(&entry) {
            Ok(meta) => meta,
            Err(_) => return WalkSize::Unknown,
        };
        // `is_symlink()` misses Windows junctions, and reparse points must never
        // be followed. The attribute is already in the metadata we just read, so
        // this used to cost a second stat per entry — which doubled the cost of
        // every scan that estimates sizes.
        if is_reparse_metadata(&meta) {
            continue;
        }
        if meta.is_dir() {
            let sub = walk_bytes_limited_quota_with_cancel(
                &entry.path(),
                depth + 1,
                entries_seen,
                quota,
                read,
                metadata,
                cancel,
            );
            match sub {
                WalkSize::Exact(sub) => total = total.saturating_add(sub),
                // A subtree that stopped on a bound still vouches for its own
                // partial sum; the parent inherits the floor.
                WalkSize::Floor(sub) => {
                    total = total.saturating_add(sub);
                    floor = true;
                }
                WalkSize::Unknown => return WalkSize::Unknown,
            }
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    if floor {
        WalkSize::Floor(total)
    } else {
        WalkSize::Exact(total)
    }
}

/// Wrap a partial total as a verified lower bound. Zero is excluded: a bound
/// of `>= 0 KB` carries no information the row did not already have, so the
/// row stays unmeasured rather than displaying a meaningless floor.
fn floor_walk(total: u64) -> WalkSize {
    if total == 0 {
        WalkSize::Unknown
    } else {
        WalkSize::Floor(total)
    }
}

/// Whether metadata describes a reparse point (junction / mount / symlink).
///
/// Reads the attribute from metadata already in hand instead of issuing a second
/// `symlink_metadata` on the path — that second stat doubled the cost of every
/// size-estimating walk. Shared with the installer scan, which reads the same
/// attribute out of the directory-entry metadata it already has.
pub(crate) fn is_reparse_metadata(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT = 0x400
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// Bounded walk returning (bytes, incomplete). Budget or IO errors make bytes a floor.
fn walk_size_bytes_with(root: &Path, cancelled: &AtomicBool) -> (u64, bool) {
    walk_size_bytes_with_reader(
        root,
        cancelled,
        &|p: &Path| std::fs::read_dir(p),
        &std::fs::DirEntry::metadata,
    )
}

fn walk_size_bytes_with_reader<F, I>(
    root: &Path,
    cancelled: &AtomicBool,
    read: &F,
    metadata: &impl Fn(&std::fs::DirEntry) -> std::io::Result<std::fs::Metadata>,
) -> (u64, bool)
where
    F: Fn(&Path) -> std::io::Result<I>,
    I: Iterator<Item = std::io::Result<std::fs::DirEntry>>,
{
    use std::collections::VecDeque;
    let mut total: u64 = 0;
    // depth + entry caps — same budget as `walk_bytes_limited`.
    let mut stack: VecDeque<(std::path::PathBuf, u32)> = VecDeque::new();
    stack.push_back((root.to_path_buf(), 0));
    let mut entries_seen: u64 = 0;
    let mut capped = false;

    while let Some((dir, depth)) = stack.pop_front() {
        if depth > MAX_WALK_DEPTH {
            continue;
        }
        // The caller's own flag (an estimate batch must not zero idle sizes) plus
        // the scan task: a cancelled scan must not keep walking either.
        if cancelled.load(Ordering::SeqCst) || crate::scan_task::cancelled() {
            return (0, false);
        }
        let Ok(entries) = read(&dir) else {
            capped = true;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                capped = true;
                continue;
            };
            if cancelled.load(Ordering::SeqCst) || crate::scan_task::cancelled() {
                return (0, false);
            }
            entries_seen += 1;
            if entries_seen >= MAX_WALK_ENTRIES {
                return (total, true);
            }
            let Ok(meta) = metadata(&entry) else {
                capped = true;
                continue;
            };
            // `is_symlink()` misses Windows junctions — check reparse attributes.
            if crate::fsutil::is_reparse_point(&entry.path()) {
                continue;
            }
            if meta.is_dir() {
                if depth < MAX_WALK_DEPTH {
                    stack.push_back((entry.path(), depth + 1));
                } else {
                    // Subtrees below the depth cap are not visited — the total
                    // is a floor and must say so.
                    capped = true;
                }
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
                if entries_seen % 512 == 0 && cancelled.load(Ordering::SeqCst) {
                    return (0, false);
                }
            }
        }
    }
    (total, capped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::{fs, io::Write};

    #[test]
    fn directory_read_errors_are_incomplete_instead_of_empty_totals() {
        let root = std::env::temp_dir().join(format!("remova-size-io-{}", std::process::id()));
        fs::write(&root, b"file, not a directory").unwrap();
        let cancel = AtomicBool::new(false);
        assert_eq!(walk_size_kb_with_capped(&root, &cancel), (0, true));
        // A plain file is exact even with a tiny quota — the quota gates
        // entries walked, not files.
        assert_eq!(walk_size_kb_with_quota(&root, 0), WalkSize::Exact(1));
        fs::remove_file(&root).unwrap();
        assert_eq!(walk_size_kb_with_capped(&root, &cancel), (0, false));
    }

    #[test]
    fn read_iterator_and_metadata_errors_never_publish_complete_sizes() {
        let root = std::env::temp_dir().join(format!("remova-size-errors-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let file = root.join("file");
        let cancel = AtomicBool::new(false);
        for mode in ["read", "iterator", "metadata"] {
            let reader = |p: &Path| -> std::io::Result<
                std::vec::IntoIter<std::io::Result<std::fs::DirEntry>>,
            > {
                if mode == "read" {
                    return Err(std::io::ErrorKind::PermissionDenied.into());
                }
                if mode == "iterator" {
                    return Ok(vec![Err(std::io::ErrorKind::PermissionDenied.into())].into_iter());
                }
                let entry = fs::read_dir(p)?.next().unwrap()?;
                Ok(vec![Ok(entry)].into_iter())
            };
            let metadata = |entry: &std::fs::DirEntry| {
                if mode == "metadata" {
                    Err(std::io::ErrorKind::PermissionDenied.into())
                } else {
                    entry.metadata()
                }
            };
            fs::write(&file, b"payload").unwrap();
            assert_eq!(
                walk_size_bytes_with_reader(&root, &cancel, &reader, &metadata),
                (0, true)
            );
            fs::write(&file, b"payload").unwrap();
            // The quota-aware core must not dress an unreadable entry up as a
            // floor, either — a read/iterator/metadata error is Unknown.
            let mut seen = 0u64;
            let unlimited = WalkQuota::new(u64::MAX);
            assert_eq!(
                walk_bytes_limited_quota_with(&root, 0, &mut seen, &unlimited, &reader, &metadata),
                WalkSize::Unknown
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn walk_sums_files_in_kb() {
        let tmp = std::env::temp_dir().join(format!("remova_dirsize_{}", std::process::id()));
        let nested = tmp.join("a/b");
        fs::create_dir_all(&nested).unwrap();
        {
            let mut f1 = fs::File::create(tmp.join("a/f1.bin")).unwrap();
            f1.write_all(&vec![0u8; 2048]).unwrap();
            f1.sync_all().unwrap();
            let mut f2 = fs::File::create(nested.join("f2.bin")).unwrap();
            f2.write_all(&vec![0u8; 1024]).unwrap();
            f2.sync_all().unwrap();
        }

        let kb = walk_size_kb_with(&tmp, &AtomicBool::new(false));
        assert_eq!(kb, 3);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// The one-player defect: a spent thread-local budget must not bleed into
    /// `walk_size_kb_unbudgeted`. A blocking-pool thread reused after a scan
    /// used to return None for every later session-size read.
    #[test]
    fn unbudgeted_walk_survives_a_spent_thread_local_budget() {
        let tmp = crate::test_support::TempDir::new("dirsize_unb");
        let session = tmp.join("1700000000_session");
        fs::create_dir_all(session.join("files")).unwrap();
        fs::write(session.join("files/fixture.bin"), vec![0u8; 2048]).unwrap();
        // Spend this thread's own budget exactly as a finished scan leaves it.
        assert!(charge_walk_budget(SIZE_WALK_ENTRY_BUDGET));
        assert!(!charge_walk_budget(1), "budget is spent");
        let kb = walk_size_kb_unbudgeted(&session);
        assert_eq!(
            kb,
            Some(2),
            "a reused thread must still measure the session"
        );
    }

    #[test]
    fn walk_missing_is_zero() {
        let missing = std::env::temp_dir().join("remova_dirsize_missing_xyz");
        assert_eq!(walk_size_kb_with(&missing, &AtomicBool::new(false)), 0);
    }

    /// hitting the file cap must flag `capped`, not pretend completeness.
    #[test]
    fn walk_file_cap_sets_capped_flag() {
        let tmp = std::env::temp_dir().join(format!("remova_dirsize_cap_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        for i in 0..MAX_WALK_ENTRIES {
            let mut f = fs::File::create(tmp.join(format!("f{i}.bin"))).unwrap();
            f.write_all(&[0u8; 8]).unwrap();
        }
        let (kb, capped) = walk_size_kb_with_capped(&tmp, &AtomicBool::new(false));
        assert!(capped, "file-cap walk must be marked partial");
        assert!(kb > 0);
        // Under the cap a complete walk is uncapped.
        let small =
            std::env::temp_dir().join(format!("remova_dirsize_small_{}", std::process::id()));
        let _ = fs::remove_dir_all(&small);
        fs::create_dir_all(&small).unwrap();
        fs::File::create(small.join("a.bin")).unwrap();
        let (_kb, capped2) = walk_size_kb_with_capped(&small, &AtomicBool::new(false));
        assert!(!capped2);
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_dir_all(&small);
    }

    #[test]
    fn walk_limited_sums_small_tree() {
        let tmp = std::env::temp_dir().join(format!("remova_dirsize_lim_{}", std::process::id()));
        fs::create_dir_all(tmp.join("sub")).unwrap();
        fs::File::create(tmp.join("a.bin"))
            .unwrap()
            .write_all(&vec![0u8; 1024])
            .unwrap();
        fs::File::create(tmp.join("sub/b.bin"))
            .unwrap()
            .write_all(&vec![0u8; 2048])
            .unwrap();
        assert_eq!(walk_size_kb_limited(&tmp), Some(3));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn empty_directory_width_consumes_the_tree_budget() {
        let root = std::env::temp_dir().join(format!(
            "remova-empty-width-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        for index in 0..MAX_WALK_ENTRIES {
            fs::create_dir(root.join(index.to_string())).unwrap();
        }
        assert_eq!(walk_size_kb_limited(&root), None);
        assert_eq!(
            walk_size_kb_with_capped(&root, &AtomicBool::new(false)),
            (0, true)
        );
        crate::fsutil::remove_tree_no_reparse(&root).unwrap();
    }

    #[test]
    fn walk_limited_missing_none() {
        let missing = std::env::temp_dir().join("remova_dirsize_lim_missing_xyz");
        assert_eq!(walk_size_kb_limited(&missing), None);
    }

    #[test]
    fn walk_limited_single_file() {
        let tmp = std::env::temp_dir().join(format!("remova_dirsize_file_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let f = tmp.join("one.bin");
        fs::File::create(&f)
            .unwrap()
            .write_all(&vec![0u8; 2500])
            .unwrap();
        assert_eq!(walk_size_kb_limited(&f), Some(3));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn walk_cancel_returns_zero() {
        let tmp = std::env::temp_dir().join(format!("remova_dirsize_c_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::File::create(tmp.join("x.bin"))
            .unwrap()
            .write_all(&[0u8; 100])
            .unwrap();
        let flag = AtomicBool::new(true);
        assert_eq!(walk_size_kb_with(&tmp, &flag), 0);
        let _ = fs::remove_dir_all(&tmp);
    }

    /// A parallel size pass shares ONE entry budget. Eight workers charging a
    /// pool of ten must produce exactly ten charges and ten refusals — the pool
    /// is never over-charged, and a worker that finishes early leaves its unused
    /// quota for the others instead of each worker getting a full budget.
    #[test]
    fn shared_walk_budget_is_one_pool_for_every_worker() {
        let pool = SharedWalkBudget::seeded(10);
        let charged = std::sync::Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let pool = pool.clone();
                let charged = charged.clone();
                scope.spawn(move || {
                    let _guard = pool.install();
                    for _ in 0..4 {
                        if charge_walk_budget(1) {
                            charged.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                });
            }
        });
        assert_eq!(charged.load(Ordering::SeqCst), 10, "the pool is one bound");
        assert_eq!(pool.remaining(), 0);
        // The guard restored the thread-local budget, so the serial path is
        // untouched by a parallel pass.
        assert_eq!(remaining_walk_budget(), SIZE_WALK_ENTRY_BUDGET);
        reset_walk_budget();
    }

    /// A background walk stops one slot short while a foreground walk is waiting,
    /// so there is always one free for it — and the restriction lifts entirely
    /// once nothing foreground is waiting, which is what keeps background walks
    /// from starving.
    #[test]
    fn background_walks_yield_a_slot_to_a_waiting_foreground_walk() {
        // Nothing foreground is waiting: background walks get every slot.
        assert_eq!(walk_limit(false, 0), MAX_CONCURRENT_WALKERS);
        // A foreground walk is waiting: background stops one short.
        assert_eq!(
            walk_limit(false, 1),
            MAX_CONCURRENT_WALKERS - FOREGROUND_RESERVED_SLOTS
        );
        // A foreground walk itself is never held back by other waiters.
        assert_eq!(walk_limit(true, 3), MAX_CONCURRENT_WALKERS);
        // The reservation must be smaller than the cap, or a waiting foreground
        // walk could never be satisfied. Pinned as a const assertion so the
        // relationship cannot be edited away silently.
        const _: () = assert!(FOREGROUND_RESERVED_SLOTS < MAX_CONCURRENT_WALKERS);
        assert!(walk_limit(false, 1) < MAX_CONCURRENT_WALKERS);
    }

    /// The cap is global, so however many walks are in flight the observed
    /// concurrency never exceeds it.
    #[test]
    fn walk_slots_are_capped() {
        let running = std::sync::Arc::new(AtomicUsize::new(0));
        let peak = std::sync::Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..(MAX_CONCURRENT_WALKERS * 3) {
                let running = running.clone();
                let peak = peak.clone();
                scope.spawn(move || {
                    let _slot = acquire_walk_slot().expect("walk slot");
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    std::thread::yield_now();
                    running.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        assert!(
            peak.load(Ordering::SeqCst) <= MAX_CONCURRENT_WALKERS,
            "observed {} concurrent walks, cap is {}",
            peak.load(Ordering::SeqCst),
            MAX_CONCURRENT_WALKERS
        );
        // The global counters are deliberately not asserted here: other tests
        // walk concurrently in the same process, so only the cap — which is
        // enforced globally — is a stable invariant.
    }

    /// PERF-B: a walk that declares itself background must not be classified as
    /// foreground — otherwise a scan's own walkers could fill the foreground
    /// reserve and squeeze real background walks out. One slot, so concurrent
    /// size-walk tests only ever have to wait briefly.
    #[test]
    fn background_acquisition_is_never_classified_as_foreground() {
        let slot = acquire_walk_slot_background().expect("background walk slot");
        // A private field, asserted from inside the module: the classification
        // rides on the slot object, not on any shared global, so parallel tests
        // cannot race this.
        assert!(
            !slot.foreground,
            "a background acquisition must not take the foreground path"
        );
        drop(slot);
    }

    /// PERF-B: a scan task's own thread (or a worker it spawned) may not grab
    /// the foreground reserve. A worker spawned inside a scan runs on a thread
    /// without the task's thread-local, so before this fix every such walker
    /// was judged foreground and the scan occupied its own reserve.
    #[test]
    fn acquire_inside_a_scan_task_is_background() {
        let id = crate::scan_task::begin().expect("test scan task");
        crate::scan_task::run(id, || {
            let slot = acquire_walk_slot().expect("walk slot inside a scan");
            assert!(
                !slot.foreground,
                "a walk inside a scan task must not take the foreground path"
            );
            drop(slot);
        })
        .expect("run inside scan task");
    }

    /// A thread that already holds a slot must never take a second one — from
    /// either entry point. A parallel sizing worker holds one slot for its
    /// whole slice and its per-row walks are nested; if the nested acquisition
    /// waited instead of returning `None`, every worker would hold one slot
    /// while waiting for the next, which deadlocked the sizing pass outright
    /// (three workers, three slots, zero progress).
    #[test]
    fn nested_acquisition_reuses_the_threads_slot() {
        let outer = acquire_walk_slot_background().expect("outer slot");
        assert!(
            acquire_walk_slot_background().is_none(),
            "a nested background walk must reuse the held slot"
        );
        assert!(
            acquire_walk_slot().is_none(),
            "a nested walk must reuse the held slot whatever its classification"
        );
        drop(outer);
    }

    /// The counterpoint of [`acquire_inside_a_scan_task_is_background`]: a plain
    /// thread with no scan task counts as foreground, so a direct size request
    /// keeps its reserve.
    #[test]
    fn acquire_outside_any_scan_task_is_foreground() {
        assert!(!crate::scan_task::has_task());
        let slot = acquire_walk_slot().expect("walk slot outside scans");
        assert!(
            slot.foreground,
            "a direct size request must take the foreground path"
        );
        drop(slot);
    }
}
