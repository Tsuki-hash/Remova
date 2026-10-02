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
        self.cleanup_at(SystemTime::now(), |path, delete_pin| {
            validate(delete_pin, true)?;
            let mut remaining = 10_000;
            for child in fs::read_dir(path).map_err(|_| "seal:stage_read")? {
                verify_tree(
                    &child.map_err(|_| "seal:stage_read")?.path(),
                    1,
                    &mut remaining,
                )?;
            }
            Ok(())
        });
    }

    fn cleanup_at(
        &self,
        now: SystemTime,
        mut verify: impl FnMut(&Path, &crate::fsutil::DirPin) -> Result<(), String>,
    ) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
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
            if verify(&path, &delete_pin).is_err() {
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
    fn another_process_pin_preserves_the_entire_active_stage() {
        use std::io::{BufRead, Write};
        use std::process::{Command, Stdio};
        const CHILD_PATH: &str = "REMOVA_STAGE_RECOVERY_CHILD_PATH";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            let _active = pin(Path::new(&path), true).unwrap();
            println!("STAGE_PIN_READY");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_line(&mut String::new()).unwrap();
            return;
        }
        let created = SystemTime::now();
        let stamp = created.duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!(
            "remova_stage_process_{}_{stamp}",
            std::process::id()
        ));
        let parents = crate::fsutil::create_dirs_pinned(&root).unwrap();
        let stage = root.join(format!("stage_1_{stamp}"));
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("snapshot"), b"other process bytes").unwrap();
        let store = Store {
            key: Zeroizing::new([0; 32]),
            root: root.clone(),
            _pins: vec![],
        };
        struct ChildGuard(std::process::Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "path_seal::key::recovery::tests::another_process_pin_preserves_the_entire_active_stage",
                    "--nocapture",
                ])
                .env(CHILD_PATH, &stage)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let mut output = std::io::BufReader::new(child.0.stdout.take().unwrap());
        let ready = (&mut output)
            .lines()
            .any(|line| line.unwrap().contains("STAGE_PIN_READY"));
        assert!(ready, "child must actually acquire the native stage pin");
        let now = created + MIN_AGE * 2;
        store.cleanup_at(now, |_, _| {
            panic!("active stage must never reach verification")
        });
        assert_eq!(
            fs::read(stage.join("snapshot")).unwrap(),
            b"other process bytes"
        );
        child
            .0
            .stdin
            .take()
            .unwrap()
            .write_all(b"release\n")
            .unwrap();
        assert!(child.0.wait().unwrap().success());
        store.cleanup_at(now, |_, _| Ok(()));
        assert!(!stage.exists(), "inactive stage must become recoverable");
        drop(parents);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_only_removes_aged_inactive_verified_directories() {
        let created = SystemTime::now();
        let stamp = created.duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!(
            "remova_stage_recovery_{}_{stamp}",
            std::process::id()
        ));
        let parents = crate::fsutil::create_dirs_pinned(&root).unwrap();
        let store = Store {
            key: Zeroizing::new([0; 32]),
            root: root.clone(),
            _pins: vec![],
        };
        let now = created + MIN_AGE * 2;
        let future = now.duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let aged = root.join(format!("stage_1_{stamp}"));
        let active_path = root.join(format!("stage_2_{stamp}"));
        let fresh = root.join(format!("stage_3_{future}"));
        let unknown = root.join("other_directory");
        for path in [&aged, &active_path, &fresh, &unknown] {
            fs::create_dir_all(path.join("nested")).unwrap();
            fs::write(path.join("nested/snapshot"), b"preserve bytes").unwrap();
        }
        for name in ["seal.key", "initialized.v2"] {
            fs::write(root.join(name), b"trusted state").unwrap();
        }
        let active = pin(&active_path, true).unwrap();
        let mut verified = vec![];
        // This isolated fixture deliberately substitutes ACL verification only.
        // Production cleanup_abandoned always supplies the real ACL/tree verifier.
        store.cleanup_at(now, |path, _| {
            verified.push(path.to_path_buf());
            Ok(())
        });
        assert!(
            !aged.exists(),
            "aged nested tree must be removed without self-lock"
        );
        assert_eq!(verified, [aged]);
        for path in [&active_path, &fresh, &unknown] {
            assert_eq!(
                fs::read(path.join("nested/snapshot")).unwrap(),
                b"preserve bytes"
            );
        }
        for name in ["seal.key", "initialized.v2"] {
            assert_eq!(fs::read(root.join(name)).unwrap(), b"trusted state");
        }
        drop(active);
        let mut rejected = vec![];
        store.cleanup_at(now, |path, deletion| {
            rejected.push(path.to_path_buf());
            validate(deletion, true)
        });
        assert_eq!(rejected.as_slice(), std::slice::from_ref(&active_path));
        assert_eq!(
            fs::read(active_path.join("nested/snapshot")).unwrap(),
            b"preserve bytes"
        );
        // A missing root is a harmless best-effort no-op.
        drop(parents);
        fs::remove_dir_all(&root).unwrap();
        store.cleanup_at(now, |_, _| panic!("missing root must not invoke verifier"));
    }

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
