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
const MAX_WALK_FILES: u64 = 5_000;
const MAX_WALK_DEPTH: u32 = 8;

/// Sum file sizes under `root` in KB (ceil). Returns 0 if missing, cancelled, or empty.
pub fn walk_size_kb(root: &Path) -> i64 {
    walk_size_kb_with(root, &CANCELLED)
}

pub fn walk_size_kb_with(root: &Path, cancelled: &AtomicBool) -> i64 {
    if !root.exists() {
        return 0;
    }
    let bytes = walk_size_bytes_with(root, cancelled);
    if bytes == 0 {
        0
    } else {
        bytes.div_ceil(1024) as i64
    }
}

/// Bounded walk for leftover items: depth + entry caps, no global cancel flag.
/// Returns None when the walk hits a cap (partial size would mislead users).
pub fn walk_size_kb_limited(root: &Path) -> Option<u64> {
    if !root.exists() {
        return None;
    }
    if root.is_file() {
        let meta = root.metadata().ok()?;
        return Some(meta.len().div_ceil(1024));
    }
    walk_bytes_limited(root, 0).map(|b| b.div_ceil(1024))
}

fn walk_bytes_limited(dir: &Path, depth: u32) -> Option<u64> {
    if depth > MAX_WALK_DEPTH {
        return None;
    }
    let mut total: u64 = 0;
    let mut files_seen: u64 = 0;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(sm) = entry.path().symlink_metadata() else {
            continue;
        };
        if sm.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let sub = walk_bytes_limited(&entry.path(), depth + 1)?;
            total = total.saturating_add(sub);
        } else if meta.is_file() {
            total = total.saturating_add(meta.len());
            files_seen += 1;
            if files_seen > MAX_WALK_FILES {
                return None;
            }
        }
    }
    Some(total)
}

fn walk_size_bytes_with(root: &Path, cancelled: &AtomicBool) -> u64 {
    use std::collections::VecDeque;
    let mut total: u64 = 0;
    // REV-SUP-03: depth + entry caps — same budget as `walk_bytes_limited`.
    let mut stack: VecDeque<(std::path::PathBuf, u32)> = VecDeque::new();
    stack.push_back((root.to_path_buf(), 0));
    let mut files_seen: u64 = 0;

    while let Some((dir, depth)) = stack.pop_front() {
        if depth > MAX_WALK_DEPTH {
            continue;
        }
        if cancelled.load(Ordering::SeqCst) {
            return 0;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if cancelled.load(Ordering::SeqCst) {
                return 0;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            // metadata() follows symlinks; use symlink_metadata to avoid loops
            let Ok(sm) = entry.path().symlink_metadata() else {
                continue;
            };
            if sm.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                if depth < MAX_WALK_DEPTH {
                    stack.push_back((entry.path(), depth + 1));
                }
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
                files_seen += 1;
                if files_seen >= MAX_WALK_FILES {
                    return total;
                }
                if files_seen % 512 == 0 && cancelled.load(Ordering::SeqCst) {
                    return 0;
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::{fs, io::Write};

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
