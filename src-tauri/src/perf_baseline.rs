//! Performance baseline. Test-only and opt-in: every case is `#[ignore]`d,
//! because a number taken beside 700 other tests says nothing about the shipped
//! build. Run it alone, in release mode:
//!
//! ```text
//! cd src-tauri
//! cargo test --release --lib perf_baseline -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `REMOVA_PERF_OUT` names a directory that receives `components.json`; without
//! it the numbers are only printed. Fixtures live under `%TEMP%` and are removed
//! on drop, including after a panic.
//!
//! What each case actually measures:
//!
//! - `walk`: `dirsize::walk_size_kb_capped` over 4,040 entries — the bounded
//!   directory-size walk the app uses to attach leftover sizes. The walk's own
//!   entry cap is 5,000, so a "10 万项" figure cannot be produced by this code
//!   path without measuring something the app never does.
//! - `analyze`: `scanner::analyze_associations` for one synthetic install.
//! - `cancel`: `scan_task::begin` → worker observes `cancelled()` after
//!   `scan_task::cancel` — the propagation the "stop" button depends on.
//! - `backup`: `backup::backup_items` for one 1 GiB file, including the seal
//!   write (DPAPI) and the publication/read-back verification.

use crate::scanner::{CleanupItem, Confidence, ItemKind, RiskLevel};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const FILE_BYTES: usize = 1024;
const BACKUP_BYTES: u64 = 1 << 30;
const WALK_ROOTS: usize = 4;
const WALK_SUBDIRS: usize = 10;
const WALK_FILES_PER_SUBDIR: usize = 100;
const ANALYZE_FILES: usize = 200;
const REPS: usize = 3;

fn elapsed_ms(start: Instant) -> f64 {
    (start.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

fn write_bytes(path: &Path, bytes: usize) {
    let mut file = std::fs::File::create(path).unwrap_or_else(|e| panic!("fixture {path:?}: {e}"));
    file.write_all(&vec![b'p'; bytes])
        .unwrap_or_else(|e| panic!("fixture {path:?}: {e}"));
}

/// `roots` × `subdirs` directories holding `files` files each. Returns the number
/// of filesystem entries the walk will visit (directories + files).
fn build_tree(root: &Path, roots: usize, subdirs: usize, files: usize) -> usize {
    let mut entries = 0;
    for r in 0..roots {
        let top = root.join(format!("root{r}"));
        std::fs::create_dir_all(&top).unwrap();
        for d in 0..subdirs {
            let dir = top.join(format!("sub{d}"));
            std::fs::create_dir_all(&dir).unwrap();
            entries += 1;
            for f in 0..files {
                write_bytes(&dir.join(format!("file{f}.bin")), FILE_BYTES);
                entries += 1;
            }
        }
    }
    entries
}

fn file_item(path: &Path) -> CleanupItem {
    CleanupItem {
        path: path.to_string_lossy().into_owned(),
        kind: ItemKind::File,
        score: 90,
        confidence: Confidence::Confirmed,
        risk: RiskLevel::Low,
        reason: "perf fixture".into(),
        evidence: vec![],
        shared: false,
        service_bound: false,
        user_data: false,
        user_library: false,
        size_kb: None,
        size_capped: false,
        bucket: None,
    }
}

/// Median/worst of `walk_size_kb_capped` over an already-built tree.
fn measure_walk(tree: &Path, reps: usize) -> (f64, f64, i64) {
    let mut samples = Vec::with_capacity(reps);
    let mut kb = 0;
    for _ in 0..reps {
        let start = Instant::now();
        let (measured, _capped) = crate::dirsize::walk_size_kb_capped(tree);
        samples.push(elapsed_ms(start));
        kb = measured;
    }
    let worst = samples.iter().copied().fold(0.0_f64, f64::max);
    (median(&mut samples), worst, kb)
}

fn measure_analyze(install: &Path, reps: usize) -> (f64, f64) {
    let install = install.to_string_lossy().into_owned();
    let mut samples = Vec::with_capacity(reps);
    for _ in 0..reps {
        let start = Instant::now();
        let report = crate::scanner::analyze_associations(
            "PerfBaselineFixture",
            &install,
            "PerfPublisher",
            "",
            "fixture",
        );
        samples.push(elapsed_ms(start));
        // Keep the optimizer honest about the scan result being consumed.
        std::hint::black_box(report.items.len());
    }
    let worst = samples.iter().copied().fold(0.0_f64, f64::max);
    (median(&mut samples), worst)
}

/// Time from `scan_task::cancel` to the worker observing `cancelled()`.
fn measure_cancel(reps: usize) -> (f64, f64) {
    let mut samples = Vec::with_capacity(reps);
    for _ in 0..reps {
        let id = crate::scan_task::begin().expect("scan task slot");
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _ = crate::scan_task::run_scoped(id, "perf-baseline", || {
                ready_tx.send(()).expect("ready channel");
                let deadline = Instant::now() + Duration::from_secs(10);
                while !crate::scan_task::cancelled() {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                done_tx.send(Instant::now()).expect("done channel");
            });
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker must start inside its task");
        // Let the worker settle into the polling loop before cancelling.
        std::thread::sleep(Duration::from_millis(20));
        let start = Instant::now();
        assert!(
            crate::scan_task::cancel(id).expect("cancel accepted"),
            "a running task must accept cancellation"
        );
        let observed = done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("worker must observe the cancellation");
        samples.push(observed.duration_since(start).as_secs_f64() * 1000.0);
        worker.join().expect("worker thread");
    }
    let worst = samples.iter().copied().fold(0.0_f64, f64::max);
    (median(&mut samples), worst)
}

/// One 1 GiB backup through the production path (native copy + seal).
fn measure_backup(root: &Path) -> (f64, u32, u32, Vec<String>) {
    let big = root.join("payload.bin");
    let mut file = std::fs::File::create(&big).expect("create backup payload");
    let chunk = vec![0x5au8; 1 << 20];
    let mut written = 0u64;
    while written < BACKUP_BYTES {
        file.write_all(&chunk).expect("write backup payload");
        written += chunk.len() as u64;
    }
    file.sync_all().expect("flush backup payload");
    drop(file);

    let session = root.join("session");
    std::fs::create_dir_all(session.join("files")).expect("session files dir");
    // Redirect the seal key/stage root into the fixture: the real store lives
    // under %ProgramData% and must not be touched by a measurement.
    let _store = crate::path_seal::TestStore::new(root.join("seals"));

    let item = file_item(&big);
    let start = Instant::now();
    let (ok, fail, errors) = crate::backup::backup_items(std::slice::from_ref(&item), &session);
    let ms = elapsed_ms(start);
    (ms, ok, fail, errors)
}

fn host_json() -> serde_json::Value {
    let os = std::env::var("OS").unwrap_or_else(|_| std::env::consts::OS.to_string());
    serde_json::json!({
        "os": os,
        "arch": std::env::consts::ARCH,
        "processors": std::env::var("NUMBER_OF_PROCESSORS").unwrap_or_default(),
        "cpu": std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),
    })
}

#[test]
#[ignore = "performance baseline: run explicitly with --ignored in release mode"]
fn p01_component_baseline() {
    let root = crate::test_support::TempDir::new("perf_baseline");
    let fixture = root.path();

    let tree = fixture.join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    let entries = build_tree(&tree, WALK_ROOTS, WALK_SUBDIRS, WALK_FILES_PER_SUBDIR);
    let (walk_median, walk_worst, walk_kb) = measure_walk(&tree, REPS);

    let install = fixture.join("install");
    std::fs::create_dir_all(&install).unwrap();
    for i in 0..ANALYZE_FILES {
        write_bytes(&install.join(format!("perfbaseline{i}.exe")), 4096);
    }
    let (analyze_median, analyze_worst) = measure_analyze(&install, REPS);

    let (cancel_median, cancel_worst) = measure_cancel(REPS);

    let (backup_ms, backup_ok, backup_fail, backup_errors) = measure_backup(fixture);
    assert_eq!(
        (backup_ok, backup_fail),
        (1, 0),
        "backup must succeed: {backup_errors:?}"
    );

    println!(
        "walk {entries} entries: median {walk_median} ms, worst {walk_worst} ms, {walk_kb} KB"
    );
    println!("analyze {ANALYZE_FILES} files: median {analyze_median} ms, worst {analyze_worst} ms");
    println!("cancel: median {cancel_median} ms, worst {cancel_worst} ms");
    println!("backup 1 GiB: {backup_ms} ms");

    if let Some(dir) = std::env::var_os("REMOVA_PERF_OUT") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("perf output dir");
        let doc = serde_json::json!({
            "kind": "remova-components",
            "generated_at": chrono_like_now(),
            "host": host_json(),
            "fixture": {
                "walk_entries": entries,
                "analyze_files": ANALYZE_FILES,
                "backup_bytes": BACKUP_BYTES,
            },
            "metrics": {
                "walk_median_ms": walk_median,
                "walk_worst_ms": walk_worst,
                "walk_kb": walk_kb,
                "analyze_median_ms": analyze_median,
                "analyze_worst_ms": analyze_worst,
                "cancel_median_ms": cancel_median,
                "cancel_worst_ms": cancel_worst,
                "backup_ms": backup_ms,
            }
        });
        let path = dir.join("components.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&doc).unwrap()).expect("write components");
        println!("wrote {}", path.display());
    }
}

/// Seconds since the epoch, formatted as an ISO-8601 UTC timestamp.
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Civil-from-days (Howard Hinnant), so no date dependency is added for a log line.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}
