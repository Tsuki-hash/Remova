//! PERF-A: per-row size sub-quota with leftover reallocation.
//!
//! Extracted from `scanner/mod.rs` so the multi-round allocation logic can be
//! tested in isolation from the scan plumbing.

use super::{CleanupItem, ItemKind};

/// The measured answer for one row.
#[derive(Debug, Clone, Copy)]
pub struct RowSize {
    pub size_kb: Option<u64>,
    /// `true` when `size_kb` is a verified lower bound that hit one of the
    /// walk's bounds (row sub-quota / pass pool / entry cap).
    pub capped: bool,
    /// `true` when the walk stopped on this row's sub-quota, so a
    /// reallocation round could still give it a meaningful larger bound.
    pub quota_limited: bool,
    /// The sub-quota this answer was measured under; used to keep the best
    /// answer across rounds instead of letting a later, poorer round overwrite
    /// an earlier floor.
    pub quota: u64,
}

pub const ROW_SIZE_UNMEASURED: RowSize = RowSize {
    size_kb: None,
    capped: false,
    quota_limited: false,
    quota: 0,
};

/// How many parallel walkers one sizing round gets.
///
/// PERF-B: this is the walk-slot cap (`dirsize::MAX_CONCURRENT_WALKERS`), not
/// one above it — the 4th worker could never get a slot and only spun without
/// doing any work.
const MAX_SIZE_WALK_WORKERS: usize = crate::dirsize::MAX_CONCURRENT_WALKERS;

/// Fill missing size_kb on file/dir items (idempotent; skips registry/path).
///
/// Bounded by an overall entry budget, so the cost of the pass does not grow
/// with the number of items (PERF-1's contract). The budget is handed out as a
/// sub-quota per row (PERF-A): `min(remaining, pool / pending.len())`, so late
/// rows get a floor too instead of the first few rows eating the whole pool,
/// and reallocation rounds hand the pool's leftovers to the rows whose walk
/// stopped only on their own sub-quota. A walk that stops on a bound publishes
/// a *verified lower bound* with `size_capped = true` — the UI shows `>= N`, so
/// a partial sum is never dressed up as exact (the size_capped contract).
///
/// The walks run on a few threads because they are IO-bound and were fully
/// serial. Every worker draws from ONE entry pool seeded with what the calling
/// thread had left, so the pass stays inside one budget however many workers
/// there are.
pub fn fill_item_sizes(items: &mut [CleanupItem]) {
    fill_item_sizes_seeded(items, None);
}

/// Test hook: run the same allocator against a small pool so fixtures can be
/// tiny while still crossing the budget (PERF-D).
#[cfg(test)]
pub fn fill_item_sizes_with_budget(items: &mut [CleanupItem], budget: u64) {
    fill_item_sizes_seeded(items, Some(budget));
}

fn fill_item_sizes_seeded(items: &mut [CleanupItem], seed: Option<u64>) {
    crate::dirsize::reset_walk_budget();
    let pending: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.size_kb.is_none() && matches!(it.kind, ItemKind::File | ItemKind::Dir))
        .map(|(i, _)| i)
        .collect();
    if pending.is_empty() {
        return;
    }
    // Both round styles draw from ONE pool for the whole pass, seeded with what
    // the calling thread had left. Serial rounds read it through the
    // thread-local already; parallel rounds install it on each worker below.
    let budget = seed.unwrap_or_else(crate::dirsize::remaining_walk_budget);
    let pool = crate::dirsize::SharedWalkBudget::seeded(budget);
    // Progress during this phase used to be invisible: `checkpoint` was never
    // called here, so the UI sat at "estimating" for the whole pass — which is
    // the longest phase when a 5 GB cache is in scope. Every 8 rows rather than
    // per row: each publish takes the progress mutex and re-renders the panel;
    // the count is rows measured so far, not entries.
    const ESTIMATE_PROGRESS_STRIDE: usize = 8;
    let mut remaining: Vec<usize> = pending.clone();
    let mut measured: Vec<RowSize> = vec![ROW_SIZE_UNMEASURED; items.len()];
    let mut sized = 0usize;
    let _serial_pool = pool.install();
    // R38-2: spawned workers have no thread-local task context, so they can
    // only observe cancellation through this watch (as the pre-extraction
    // walker in scanner/mod.rs did).
    let cancel = crate::scan_task::cancel_watch();
    while !remaining.is_empty() && !cancel.cancelled() {
        let pool_left = crate::dirsize::remaining_walk_budget();
        if pool_left == 0 {
            // The pass budget is spent: rows that never finished keep whatever
            // they reported so far; the rest stay unmeasured, exactly like the
            // serial pass used to.
            break;
        }
        // This round's per-row sub-quota: an even share of what the pool still
        // holds, floored above zero. Rows that stop only on that share come
        // back next round for a slice of whatever is left.
        let round_quota = (pool_left / remaining.len() as u64).max(1);
        let before = remaining.len();
        let results = if workers_for(before) <= 1 {
            walk_rows_serial(
                &remaining,
                items,
                round_quota,
                sized,
                ESTIMATE_PROGRESS_STRIDE,
                &cancel,
            )
        } else {
            walk_rows_parallel(&remaining, items, round_quota, &pool, &cancel)
        };
        sized += before;
        let mut next_round = Vec::with_capacity(before);
        for (i, row) in results {
            // Only a re-walk under a strictly bigger quota can improve a row's
            // answer; a smaller or equal share would just repeat it (and
            // re-charge entries the pool has already paid for).
            if row.quota > measured[i].quota {
                measured[i] = row;
                if row.quota_limited {
                    next_round.push(i);
                }
            }
        }
        // R38-6: a re-queued row that would get the same (or smaller) share
        // next round can only repeat its walk — and an entry-cap stop is never
        // improved by a bigger quota anyway. Drop those before they charge the
        // pool a second time.
        if !next_round.is_empty() {
            let left = crate::dirsize::remaining_walk_budget();
            let next_quota = (left / next_round.len() as u64).max(1);
            next_round.retain(|&i| measured[i].quota < next_quota);
        }
        // Round progress: the serial path already published its stride while
        // walking; the parallel path reports once, at the round's end.
        if workers_for(before) > 1 {
            crate::scan_task::checkpoint("sizing", sized.min(pending.len()));
        }
        remaining = next_round;
    }
    // Publish in caller order so `items` stays the single owner of the answer.
    for &i in &pending {
        let row = &measured[i];
        if let Some(kb) = row.size_kb {
            items[i].size_kb = Some(kb);
            items[i].size_capped = row.capped;
        }
    }
    // A cancelled pass has not finished every row; reporting `pending.len()`
    // would tell the UI 100% (A3). Already-measured rows are verified floors
    // and stay published.
    let reported = if cancel.cancelled() {
        sized.min(pending.len())
    } else {
        pending.len()
    };
    crate::scan_task::checkpoint("sizing", reported);
}

fn workers_for(rows: usize) -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, MAX_SIZE_WALK_WORKERS)
        .min(rows)
}

fn walk_rows_serial(
    rows: &[usize],
    items: &[CleanupItem],
    quota: u64,
    sized: usize,
    progress_stride: usize,
    cancel: &crate::scan_task::CancelWatch,
) -> Vec<(usize, RowSize)> {
    let mut out = Vec::with_capacity(rows.len());
    for (done, &i) in rows.iter().enumerate() {
        if cancel.cancelled() {
            break;
        }
        out.push((i, walk_row(&items[i].path, quota)));
        if (done + 1) % progress_stride == 0 || done + 1 == rows.len() {
            crate::scan_task::checkpoint("sizing", sized + done + 1);
        }
    }
    out
}

/// One parallel round: the rows are chunked across walkers, each holding one
/// walk slot and the pass pool for its slice. Workers have no thread-local
/// task context, so cancellation travels through the watch (R38-2).
fn walk_rows_parallel(
    rows: &[usize],
    items: &[CleanupItem],
    quota: u64,
    pool: &crate::dirsize::SharedWalkBudget,
    cancel: &crate::scan_task::CancelWatch,
) -> Vec<(usize, RowSize)> {
    let workers = workers_for(rows.len());
    let chunk = rows.len().div_ceil(workers);
    let shared: &[CleanupItem] = items;
    let mut measured: Vec<(usize, RowSize)> = Vec::with_capacity(rows.len());
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for slice in rows.chunks(chunk) {
            let pool = pool.clone();
            let cancel = cancel.clone();
            handles.push(scope.spawn(move || {
                // One walk slot (declared background — PERF-B) and the shared
                // pool for the whole slice.
                let _slot = crate::dirsize::acquire_walk_slot_background();
                let _budget = pool.install();
                slice
                    .iter()
                    .filter_map(|&i| {
                        if cancel.cancelled() {
                            return None;
                        }
                        Some((i, walk_row(&shared[i].path, quota)))
                    })
                    .collect::<Vec<_>>()
            }));
        }
        for handle in handles {
            measured.extend(handle.join().unwrap_or_default());
        }
    });
    measured
}

/// The measured answer for one row's walk under one sub-quota. The pass-entry
/// pool must already be installed on the calling thread.
fn walk_row(path: &str, quota: u64) -> RowSize {
    // PERF-B: every walk this pass issues declares itself background, so the
    // scan never occupies the foreground-reserved walk slot itself.
    let (size_kb, capped, quota_limited) =
        match crate::dirsize::walk_size_kb_with_quota_bg(std::path::Path::new(path), quota, true) {
            crate::dirsize::WalkSize::Exact(kb) => (Some(kb), false, false),
            crate::dirsize::WalkSize::Floor(kb) => (
                Some(kb),
                true,
                // A quota stop can improve in a later round; an entry-cap stop
                // would not, but the walk would have to re-walk thousands of
                // entries to tell the two apart, so it optimistically re-queues
                // and the strictly-bigger-quota check on the next round decides.
                true,
            ),
            crate::dirsize::WalkSize::Unknown => (None, false, false),
        };
    RowSize {
        size_kb,
        capped,
        quota_limited,
        quota,
    }
}

#[cfg(test)]
#[path = "fill_sizes/tests.rs"]
mod tests;
