//! Directory size walk for apps missing registry EstimatedSize.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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
const MAX_WALK_ENTRIES: u64 = 5_000;
const MAX_WALK_DEPTH: u32 = 8;

/// Sum file sizes under `root` in KB (ceil). Returns 0 if missing, cancelled, or empty.
pub fn walk_size_kb(root: &Path) -> i64 {
    walk_size_kb_with(root, &CANCELLED)
}

/// Like [`walk_size_kb`] but also reports an incomplete walk (budget or IO errors).
/// Honors the global cancel flag.
pub fn walk_size_kb_capped(root: &Path) -> (i64, bool) {
    walk_size_kb_with_capped(root, &CANCELLED)
}

/// Sum file sizes under `root` in KB (ceil). Returns 0 if missing, cancelled, or empty.
/// Second element is `true` for budget or IO truncation: size is then a lower bound.
pub fn walk_size_kb_with_capped(root: &Path, cancelled: &AtomicBool) -> (i64, bool) {
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
pub fn walk_size_kb_limited(root: &Path) -> Option<u64> {
    if crate::scan_task::cancelled() {
        return None;
    }
    if !root.exists() {
        return None;
    }
    if root.is_file() {
        let meta = root.metadata().ok()?;
        return Some(meta.len().div_ceil(1024));
    }
    let mut entries_seen = 0u64;
    walk_bytes_limited(root, 0, &mut entries_seen).map(|b| b.div_ceil(1024))
}

fn walk_bytes_limited(dir: &Path, depth: u32, entries_seen: &mut u64) -> Option<u64> {
    walk_bytes_limited_with(
        dir,
        depth,
        entries_seen,
        &|p: &Path| std::fs::read_dir(p),
        &std::fs::DirEntry::metadata,
    )
}

fn walk_bytes_limited_with<F, I>(
    dir: &Path,
    depth: u32,
    entries_seen: &mut u64,
    read: &F,
    metadata: &impl Fn(&std::fs::DirEntry) -> std::io::Result<std::fs::Metadata>,
) -> Option<u64>
where
    F: Fn(&Path) -> std::io::Result<I>,
    I: Iterator<Item = std::io::Result<std::fs::DirEntry>>,
{
    if depth > MAX_WALK_DEPTH || crate::scan_task::cancelled() {
        return None;
    }
    let mut total: u64 = 0;
    let entries = read(dir).ok()?;
    for entry in entries {
        if crate::scan_task::cancelled() {
            return None;
        }
        let entry = entry.ok()?;
        *entries_seen += 1;
        if *entries_seen >= MAX_WALK_ENTRIES {
            return None;
        }
        let meta = metadata(&entry).ok()?;
        // `is_symlink()` misses Windows junctions — check reparse attributes.
        if crate::fsutil::is_reparse_point(&entry.path()) {
            continue;
        }
        if meta.is_dir() {
            let sub =
                walk_bytes_limited_with(&entry.path(), depth + 1, entries_seen, read, metadata)?;
            total = total.saturating_add(sub);
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
        }
    }
    Some(total)
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
        if cancelled.load(Ordering::SeqCst) {
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
            if cancelled.load(Ordering::SeqCst) {
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
        assert_eq!(walk_bytes_limited(&root, 0, &mut 0), None);
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
            assert_eq!(
                walk_bytes_limited_with(&root, 0, &mut 0, &reader, &metadata),
                None
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
}
