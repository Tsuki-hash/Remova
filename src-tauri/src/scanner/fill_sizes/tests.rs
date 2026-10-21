//! Tests for the per-row sub-quota allocation (PERF-A). Kept next to the
//! implementation but behind `#[cfg(test)]` via `mod tests;` in `fill_sizes.rs`.

use super::*;
use crate::scanner::{CleanupItem, Confidence, ItemKind, RiskLevel};

fn dir_item(path: &std::path::Path) -> CleanupItem {
    CleanupItem {
        path: path.to_string_lossy().to_string(),
        kind: ItemKind::Dir,
        score: 90,
        confidence: Confidence::Confirmed,
        risk: RiskLevel::Low,
        reason: "test".into(),
        evidence: vec![],
        shared: false,
        user_data: false,
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: None,
    }
}

/** Twelve small trees must all be measured exactly; a walk past the
 * per-directory entry cap stays `None`. */
#[test]
fn parallel_size_walks_report_exact_sizes() {
    let root = crate::test_support::TempDir::new("par-size");
    let mut items = Vec::new();
    for i in 0..12 {
        let dir = root.join(format!("tree{i:02}"));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("a.bin"), vec![0u8; 1024]).unwrap();
        std::fs::write(dir.join("nested").join("b.bin"), vec![0u8; 2048]).unwrap();
        items.push(dir_item(&dir));
    }
    // A tree past the per-directory entry cap must stay unknown.
    let capped = root.join("capped");
    std::fs::create_dir_all(&capped).unwrap();
    for i in 0..(crate::dirsize::MAX_WALK_ENTRIES + 8) {
        std::fs::write(capped.join(format!("f{i}.bin")), b"").unwrap();
    }
    items.push(dir_item(&capped));

    fill_item_sizes(&mut items);

    for (i, it) in items.iter().take(12).enumerate() {
        assert_eq!(it.size_kb, Some(3), "tree{i:02} must be measured exactly");
        assert!(!it.size_capped, "tree{i:02} is exact, not a floor");
    }
    assert_eq!(
        items[12].size_kb, None,
        "a walk that hit its cap must not publish a partial sum as exact"
    );
}

/** PERF-D: a small pool and 41-entry trees exercise the same allocation
 * contract as the old 20,500-entry fixture, at ~1/100 of the file churn. */
#[test]
fn parallel_size_walks_share_one_entry_budget() {
    const BUDGET: u64 = 200;
    let root = crate::test_support::TempDir::new("par-budget");
    let per_tree = 41usize;
    let mut items = Vec::new();
    for t in 0..5 {
        let dir = root.join(format!("tree{t}"));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..per_tree {
            std::fs::write(dir.join(format!("f{i:05}.bin")), vec![0u8; 1024]).unwrap();
        }
        items.push(dir_item(&dir));
    }

    fill_item_sizes_with_budget(&mut items, BUDGET);

    // A walk is only charged for entries it actually read (regardless of how
    // it was split across rows), so the full budget can fund four 41-entry
    // trees, plus part of a fifth as a floor. The floor is still a real answer
    // as long as `size_capped` says so.
    let completed = items
        .iter()
        .filter(|it| it.size_kb.is_some() && !it.size_capped)
        .count();
    assert!(
        completed <= (BUDGET as usize) / per_tree,
        "{completed} complete walks of {per_tree} entries each exceed the {BUDGET}-entry budget"
    );
    // The pass installs its own pool and restores the thread-local afterwards;
    // a leaked pool would leave the thread with someone else's budget.
    assert_eq!(
        crate::dirsize::remaining_walk_budget(),
        crate::dirsize::SIZE_WALK_ENTRY_BUDGET,
        "the pass must restore the thread-local budget"
    );
}

/** A3: the real R38-2 regression. Workers have no thread-local task context;
 * a cancelled watch must stop them BEFORE any walk starts. Reverting the
 * worker to `scan_task::cancelled()` makes this test red. */
#[test]
fn parallel_workers_stop_on_a_cancelled_watch() {
    let root = crate::test_support::TempDir::new("par-watch");
    let mut items = Vec::new();
    for t in 0..4 {
        let dir = root.join(format!("tree{t}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.bin"), vec![0u8; 1024]).unwrap();
        items.push(dir_item(&dir));
    }
    let rows: Vec<usize> = (0..items.len()).collect();
    let pool = crate::dirsize::SharedWalkBudget::seeded(1_000);
    let measured = walk_rows_parallel(
        &rows,
        &items,
        100,
        &pool,
        &crate::scan_task::cancelled_watch_for_test(),
    );
    assert!(
        measured.is_empty(),
        "a cancelled watch must stop every worker before it walks: {measured:?}"
    );
}

/** R38-2: a cancelled scan must not keep measuring. Workers have no
 * thread-local task context, so the pass must consult the cancel watch —
 * both in its loop condition and inside every spawned worker. */
#[test]
fn cancelled_scan_publishes_no_sizes() {
    let root = crate::test_support::TempDir::new("par-cancel");
    let mut items = Vec::new();
    for t in 0..4 {
        let dir = root.join(format!("tree{t}"));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..16 {
            std::fs::write(dir.join(format!("f{i:03}.bin")), vec![0u8; 1024]).unwrap();
        }
        items.push(dir_item(&dir));
    }

    let id = crate::scan_task::begin().unwrap();
    let outcome = crate::scan_task::run(id, || {
        crate::scan_task::cancel(id).unwrap();
        fill_item_sizes(&mut items);
    });
    assert!(outcome.is_err(), "the task was cancelled before the pass");
    for (t, it) in items.iter().enumerate() {
        assert!(
            it.size_kb.is_none(),
            "tree{t}: a cancelled pass must not publish a size"
        );
    }
}

/** The PERF-A fix: with twelve 49-entry trees (1 KB each) against a 500-entry
 * budget, every row must publish *something* (an exact number or a marked
 * floor) — the pre-fix pass left ~58% of rows at `None`. */
#[test]
fn subquota_gives_every_row_a_size_or_marked_floor() {
    const BUDGET: u64 = 500;
    const PER_TREE: usize = 49;
    let root = crate::test_support::TempDir::new("subquota");
    let mut items = Vec::new();
    for t in 0..12 {
        let dir = root.join(format!("tree{t:02}"));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..PER_TREE {
            std::fs::write(dir.join(format!("f{i:05}.bin")), vec![0u8; 1024]).unwrap();
        }
        items.push(dir_item(&dir));
    }

    fill_item_sizes_with_budget(&mut items, BUDGET);

    let unmeasured = items.iter().filter(|it| it.size_kb.is_none()).count();
    assert!(
        unmeasured == 0,
        "{unmeasured}/12 rows published no size at all — the sub-quota must give every row a floor"
    );
    // Every row that got a floor is marked capped — never shown as exact.
    for (t, it) in items.iter().enumerate() {
        if it.size_capped {
            assert!(
                it.size_kb.unwrap() < PER_TREE as u64,
                "tree{t:02}: a capped row's floor must be below the true total"
            );
        } else {
            assert_eq!(
                it.size_kb,
                Some(PER_TREE as u64),
                "tree{t:02} finished exactly"
            );
        }
    }
}
