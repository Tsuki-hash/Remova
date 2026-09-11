//! Directory size walk for apps missing registry EstimatedSize.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Request cancellation of in-flight / queued size walks.
pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// Clear cancel flag before starting a new estimate batch/job.
pub fn clear_cancel() {
    CANCELLED.store(false, Ordering::SeqCst);
}

/// Sum file sizes under `root` in KB (ceil). Returns 0 if missing, cancelled, or empty.
pub fn walk_size_kb(root: &Path) -> i64 {
    walk_size_kb_with(root, &CANCELLED)
}

pub fn walk_size_kb_with(root: &Path, cancelled: &AtomicBool) -> i64 {
    if !root.exists() {
        return 0;
    }
    let bytes = walk_size_bytes_with(root, cancelled);
    if bytes <= 0 {
        0
    } else {
        ((bytes + 1023) / 1024) as i64
    }
}

fn walk_size_bytes_with(root: &Path, cancelled: &AtomicBool) -> u64 {
    use std::collections::VecDeque;
    let mut total: u64 = 0;
    let mut stack: VecDeque<std::path::PathBuf> = VecDeque::new();
    stack.push_back(root.to_path_buf());
    let mut files_seen: u64 = 0;

    while let Some(dir) = stack.pop_front() {
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
                stack.push_back(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
                files_seen += 1;
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
    fn walk_cancel_returns_zero() {
        let tmp = std::env::temp_dir().join(format!("remova_dirsize_c_{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::File::create(tmp.join("x.bin"))
            .unwrap()
            .write_all(&vec![0u8; 100])
            .unwrap();
        let flag = AtomicBool::new(true);
        assert_eq!(walk_size_kb_with(&tmp, &flag), 0);
        let _ = fs::remove_dir_all(&tmp);
    }
}
