//! Microbenchmarks for hot pure guards on the scan/cleanup paths.
//! Dev-only: run with `cargo bench` inside `src-tauri`; CI compiles them
//! via clippy --all-targets but does not run them.
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use remova_lib::fsutil::is_local_drive_path;
use remova_lib::safety::is_safe_to_delete_registry;

fn bench_hot_paths(c: &mut Criterion) {
    let mut group = c.benchmark_group("hot_paths");
    group.throughput(criterion::Throughput::Elements(1));

    // Drive/locality classification: runs once per probed candidate path.
    group.bench_function("is_local_drive_path/local", |b| {
        b.iter(|| is_local_drive_path(black_box(r"C:\Program Files\Demo\app.exe")))
    });
    group.bench_function("is_local_drive_path/unc", |b| {
        b.iter(|| is_local_drive_path(black_box(r"\\server\share\app.exe")))
    });
    group.bench_function("is_local_drive_path/relative", |b| {
        b.iter(|| is_local_drive_path(black_box(r"data\cache\app.exe")))
    });

    // Delete-shape rules: run once per candidate registry target.
    group.bench_function("is_safe_to_delete_registry/product_key", |b| {
        b.iter(|| is_safe_to_delete_registry(black_box(r"HKLM64\SOFTWARE\Microsoft\Edge")))
    });
    group.bench_function("is_safe_to_delete_registry/vendor_root_refused", |b| {
        b.iter(|| is_safe_to_delete_registry(black_box(r"HKLM\SOFTWARE\Microsoft")))
    });
    group.bench_function("is_safe_to_delete_registry/muicache_refused", |b| {
        b.iter(|| {
            is_safe_to_delete_registry(black_box(
                r"HKCU\SOFTWARE\CLASSES\LOCAL SETTINGS\SOFTWARE\MICROSOFT\WINDOWS\SHELL\MUICACHE",
            ))
        })
    });

    group.finish();
}

criterion_group!(benches, bench_hot_paths);
criterion_main!(benches);
