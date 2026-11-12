//! Best-effort recovery of private staging left behind by abnormal process exit.
use super::*;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MIN_AGE: Duration = Duration::from_secs(24 * 60 * 60);

fn process_exited(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };

    match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) } {
        Ok(handle) => {
            let signalled = unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
            let _ = unsafe { CloseHandle(handle) };
            signalled
        }
        // A missing PID is conclusive. Access denied or PID reuse may mean
        // the creator still owns this stage, so keep the normal age window.
        Err(error) => error.code() == windows::core::HRESULT::from_win32(ERROR_INVALID_PARAMETER.0),
    }
}

fn candidate(name: &str, now: SystemTime) -> Option<Duration> {
    let (pid, stamp) = name.strip_prefix("stage_")?.split_once('_')?;
    if pid.is_empty()
        || stamp.is_empty()
        || !pid.bytes().chain(stamp.bytes()).all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let pid = pid.parse::<u32>().ok().filter(|pid| *pid > 0)?;
    let stamp = stamp.parse::<u64>().ok()?;
    let created = UNIX_EPOCH.checked_add(Duration::from_nanos(stamp))?;
    let age = now.duration_since(created).ok()?;
    if age >= MIN_AGE {
        Some(MIN_AGE)
    } else if process_exited(pid) {
        Some(Duration::ZERO)
    } else {
        None
    }
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
            let mut remaining = crate::path_seal::MAX_PAYLOAD_NODES * 2;
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
        // Private restore receipts share this store. Only abandoned stage
        // candidates count against the bound on staging recovery work.
        for (entry, minimum_age) in entries
            .flatten()
            .filter_map(|entry| {
                let minimum_age = candidate(entry.file_name().to_str()?, now)?;
                Some((entry, minimum_age))
            })
            .take(256)
        {
            let path = entry.path();
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
                    .is_some_and(|age| age >= minimum_age)
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
        const CHILD_STAMP: &str = "REMOVA_STAGE_RECOVERY_CHILD_STAMP";
        if let Some(root) = std::env::var_os(CHILD_PATH) {
            let stamp = std::env::var(CHILD_STAMP).unwrap().parse::<u64>().unwrap();
            let stage = PathBuf::from(root).join(format!("stage_{}_{stamp}", std::process::id()));
            fs::create_dir(&stage).unwrap();
            let _active = pin(&stage, true).unwrap();
            fs::write(stage.join("snapshot"), b"other process bytes").unwrap();
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
        for index in 0..256 {
            fs::write(
                root.join(format!("restore-{index:064x}.receipt")),
                b"retry state",
            )
            .unwrap();
        }
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
                .env(CHILD_PATH, &root)
                .env(CHILD_STAMP, stamp.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stage = root.join(format!("stage_{}_{stamp}", child.0.id()));
        let mut output = std::io::BufReader::new(child.0.stdout.take().unwrap());
        let ready = (&mut output)
            .lines()
            .any(|line| line.unwrap().contains("STAGE_PIN_READY"));
        assert!(ready, "child must actually acquire the native stage pin");
        store.cleanup_at(SystemTime::now(), |_, _| {
            panic!("a recently created stage cannot be cleaned while its creator lives")
        });
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
        store.cleanup_at(SystemTime::now(), |_, _| Ok(()));
        assert!(!stage.exists(), "inactive stage must become recoverable");
        assert!(root.join(format!("restore-{:064x}.receipt", 0)).is_file());
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
        let future = (now + Duration::from_secs(1))
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
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
    fn only_valid_past_stage_names_are_candidates() {
        let now = UNIX_EPOCH + MIN_AGE + Duration::from_nanos(1000);
        assert_eq!(candidate("stage_1_1000", now), Some(MIN_AGE));
        for name in [
            "seal.key",
            "initialized.v2",
            "stage_0_1",
            "stage_1_90000000000000",
            "stage_1_-1",
            "stage_1_1_extra",
            "stage_4294967296_1",
        ] {
            assert_eq!(candidate(name, now), None, "{name}");
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
