//! Best-effort recovery of private staging left behind by abnormal process exit.
use super::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MIN_AGE: Duration = Duration::from_secs(24 * 60 * 60);

fn candidate(name: &str, now: SystemTime) -> bool {
    let Some(rest) = name.strip_prefix("stage_") else {
        return false;
    };
    let Some((pid, stamp)) = rest.split_once('_') else {
        return false;
    };
    if pid.is_empty()
        || stamp.is_empty()
        || !pid.bytes().chain(stamp.bytes()).all(|b| b.is_ascii_digit())
    {
        return false;
    }
    if !pid.parse::<u32>().is_ok_and(|pid| pid > 0) {
        return false;
    }
    let Ok(stamp) = stamp.parse::<u64>() else {
        return false;
    };
    UNIX_EPOCH
        .checked_add(Duration::from_nanos(stamp))
        .and_then(|created| now.duration_since(created).ok())
        .is_some_and(|age| age >= MIN_AGE)
}

fn verify_tree(path: &Path, depth: usize, remaining: &mut usize) -> Result<(), String> {
    if depth > 64 || *remaining == 0 {
        return Err("seal:stage_limit".into());
    }
    *remaining -= 1;
    let meta = fs::symlink_metadata(path).map_err(|_| "seal:stage_metadata")?;
    let child = pin(path, meta.is_dir())?;
    validate_object(&child, true, false)?;
    if meta.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| "seal:stage_read")? {
            verify_tree(
                &entry.map_err(|_| "seal:stage_read")?.path(),
                depth + 1,
                remaining,
            )?;
        }
    }
    Ok(())
}

impl Store {
    pub(in crate::path_seal) fn cleanup_abandoned(&self) {
        // Synthetic protocol stores do not establish the production ACL trust boundary.
        #[cfg(test)]
        if self._pins.is_empty() {
            return;
        }
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.take(256).flatten() {
            let path = entry.path();
            if !entry
                .file_name()
                .to_str()
                .is_some_and(|name| candidate(name, now))
            {
                continue;
            }
            // Request DELETE before even reading children. An active Stage denies this
            // sharing request, including when another process holds its staging pin.
            let Ok(delete_pin) = crate::fsutil::pin_private_delete(&path) else {
                continue;
            };
            if validate(&delete_pin, true).is_err() {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !meta.is_dir()
                || !meta
                    .created()
                    .ok()
                    .and_then(|created| now.duration_since(created).ok())
                    .is_some_and(|age| age >= MIN_AGE)
            {
                continue;
            }
            let verified = (|| {
                let mut remaining = 10_000;
                for child in fs::read_dir(&path).map_err(|_| "seal:stage_read")? {
                    verify_tree(
                        &child.map_err(|_| "seal:stage_read")?.path(),
                        1,
                        &mut remaining,
                    )?;
                }
                Ok::<_, String>(())
            })();
            if verified.is_err() {
                continue;
            }
            // Store retains every trusted ancestor and key pin throughout removal.
            let _ = crate::fsutil::remove_tree_with_pin(&path, delete_pin);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_aged_stage_names_are_candidates() {
        let now = UNIX_EPOCH + MIN_AGE + Duration::from_nanos(1000);
        assert!(candidate("stage_1_1000", now));
        for name in [
            "seal.key",
            "initialized.v2",
            "stage_0_1",
            "stage_1_2000",
            "stage_1_-1",
            "stage_1_1_extra",
            "stage_4294967296_1",
        ] {
            assert!(!candidate(name, now), "{name}");
        }
    }

    #[test]
    fn native_stage_pin_blocks_cleanup_before_any_child_is_removed() {
        let root = std::env::temp_dir().join(format!(
            "remova_stage_pin_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("snapshot");
        fs::write(&path, b"active bytes").unwrap();
        let active = pin(&root, true).unwrap();
        assert!(crate::fsutil::pin_private_delete(&root).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"active bytes");
        drop(active);
        let deletion = crate::fsutil::pin_private_delete(&root).unwrap();
        assert!(
            validate(&deletion, true).is_err(),
            "ordinary temp ACL must not become trusted"
        );
        crate::fsutil::remove_tree_with_pin(&root, deletion).unwrap();
        assert!(
            !root.exists(),
            "held deletion pin must not self-lock traversal"
        );
    }
}
