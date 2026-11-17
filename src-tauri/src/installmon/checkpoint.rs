//! The persisted baseline supports read-only comparison after a restart.
//! Only a snapshot captured and held by the live process can arm cleanup.

use super::FullSnapshot;
use std::io::ErrorKind;
use std::path::PathBuf;

const READ_FAILED: &str = "monitor:checkpoint_unavailable";
const WRITE_FAILED: &str = "monitor:checkpoint_write_failed";

pub(super) struct Checkpoint {
    path: PathBuf,
    context: String,
}

impl Checkpoint {
    #[cfg(windows)]
    pub(super) fn current() -> Result<Self, String> {
        use windows::Win32::UI::Shell::FOLDERID_LocalAppData;
        let root = crate::sysops::lookup_known_folder(&FOLDERID_LocalAppData).ok_or(READ_FAILED)?;
        let sid = crate::sysops::current_user_sid().ok_or(READ_FAILED)?;
        let elevated = crate::sysops::is_process_elevated().map_err(|_| READ_FAILED)?;
        Ok(Self {
            path: root.join("Remova").join("install-monitor.v1"),
            context: format!("Remova install tracking v1:{sid}:{elevated}"),
        })
    }

    pub(super) fn exists(&self) -> Result<bool, String> {
        match self.path.symlink_metadata() {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(_) => Err(READ_FAILED.into()),
        }
    }

    #[cfg(windows)]
    pub(super) fn load(&self) -> Result<Option<FullSnapshot>, String> {
        use std::io::Read;
        if !self.exists()? {
            return Ok(None);
        }
        let file = crate::fsutil::open_local_regular_file(&self.path).map_err(|_| READ_FAILED)?;
        let mut sealed = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut sealed)
            .map_err(|_| READ_FAILED)?;
        let raw = protect(&sealed, &self.context, false).map_err(|_| READ_FAILED)?;
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|_| READ_FAILED.into())
    }

    #[cfg(windows)]
    pub(super) fn save(&self, snapshot: &FullSnapshot) -> Result<(), String> {
        use std::io::Write;
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};

        let raw = serde_json::to_vec(snapshot).map_err(|_| WRITE_FAILED)?;
        let sealed = protect(&raw, &self.context, true).map_err(|_| WRITE_FAILED)?;
        let parent = self.path.parent().ok_or(WRITE_FAILED)?;
        let _pins = crate::fsutil::create_dirs_pinned(parent).map_err(|_| WRITE_FAILED)?;
        if self.exists()? {
            return Err("monitor:checkpoint_pending".into());
        }
        let (tmp, mut file) =
            crate::fsutil::create_atomic_tmp(&self.path).map_err(|_| WRITE_FAILED)?;
        let write = file.write_all(&sealed).and_then(|_| file.sync_all());
        drop(file);
        let result = write.map_err(|_| WRITE_FAILED.to_string()).and_then(|_| {
            let wide =
                |p: &std::path::Path| p.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
            let from = wide(&tmp);
            let to = wide(&self.path);
            // Publish only if another process has not committed a baseline already.
            unsafe {
                MoveFileExW(
                    PCWSTR(from.as_ptr()),
                    PCWSTR(to.as_ptr()),
                    MOVEFILE_WRITE_THROUGH,
                )
            }
            .map_err(|_| {
                if self.exists() == Ok(true) {
                    "monitor:checkpoint_pending".into()
                } else {
                    WRITE_FAILED.into()
                }
            })
        });
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }

    pub(super) fn remove(&self) -> Result<(), String> {
        if !self.exists()? {
            return Ok(());
        }
        let parent = self.path.parent().ok_or(WRITE_FAILED)?;
        let _pins = crate::fsutil::pin_local_scan_path(parent).map_err(|_| WRITE_FAILED)?;
        // Unlink the entry itself, including a corrupt or replaced leaf.
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(_) => Err(WRITE_FAILED.into()),
        }
    }
}

#[cfg(windows)]
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[cfg(windows)]
fn protect(input: &[u8], context: &str, encrypt: bool) -> Result<Vec<u8>, ()> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    if input.is_empty() || input.len() > MAX_BYTES {
        return Err(());
    }
    let blob = |bytes: &[u8]| CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let input = blob(input);
    let entropy = blob(context.as_bytes());
    let mut out = CRYPT_INTEGER_BLOB::default();
    // DPAPI binds the data to the Windows profile. The additional context
    // prevents accidental cross-token reads. Neither property grants cleanup authority.
    unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                windows::core::w!("Remova install tracking"),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .map_err(|_| ())?;
        let valid = !out.pbData.is_null() && (out.cbData as usize) <= MAX_BYTES;
        let bytes =
            valid.then(|| std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec());
        let _ = LocalFree(HLOCAL(out.pbData.cast()));
        bytes.ok_or(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_round_trip_blocks_other_token_context_and_corrupt_bytes() {
        let lab = crate::test_support::TempDir::new("monitor_checkpoint");
        let mut checkpoint = Checkpoint {
            path: lab.path().join("checkpoint"),
            context: "same SID + regular".into(),
        };
        assert!(checkpoint.load().unwrap().is_none());
        let snapshot = FullSnapshot {
            fs: Default::default(),
            reg: vec!["fixture".into()],
            reg_degraded: false,
        };
        checkpoint.save(&snapshot).unwrap();
        assert_eq!(checkpoint.load().unwrap().unwrap().reg, ["fixture"]);
        assert_eq!(
            checkpoint.save(&snapshot),
            Err("monitor:checkpoint_pending".into())
        );
        let context = checkpoint.context.clone();
        for other in ["same SID + elevated", "different SID + regular"] {
            checkpoint.context = other.into();
            assert!(checkpoint.load().is_err());
        }
        checkpoint.context = context;
        std::fs::write(&checkpoint.path, b"forged").unwrap();
        assert!(checkpoint.load().is_err());
        checkpoint.remove().unwrap();
        assert!(checkpoint.load().unwrap().is_none());
    }

    #[test]
    fn simulated_restart_can_review_but_never_authorizes_cleanup() {
        use crate::installmon::{begin_session, end_session, FsSnapshot};
        use std::collections::BTreeSet;
        use std::sync::Mutex;

        let _allow = crate::scan_allow::test_lock();
        let lab = crate::test_support::TempDir::new("monitor_restart");
        let file = Checkpoint {
            path: lab.path().join("checkpoint"),
            context: "fixture profile".into(),
        };
        let original = Mutex::new(None);
        let capture = || FullSnapshot {
            fs: FsSnapshot::default(),
            reg: vec!["existing".into()],
            reg_degraded: false,
        };
        begin_session(
            &original,
            capture,
            || file.exists(),
            |snapshot| file.save(snapshot),
        )
        .unwrap();
        drop(original);
        let restarted = Mutex::new(None);
        assert_eq!(
            begin_session(
                &restarted,
                || panic!("would replace the pending snapshot"),
                || file.exists(),
                |_| Ok(())
            ),
            Err("monitor:checkpoint_pending".into())
        );
        let resume = || {
            end_session(
                &restarted,
                FsSnapshot::default,
                || Ok(BTreeSet::from(["existing".into(), "after".into()])),
                || file.load(),
                || file.remove(),
            )
        };
        let id = crate::scan_task::begin().unwrap();
        let result = crate::scan_task::run_scoped(id, "monitor", || {
            crate::scan_task::cancel(id).unwrap();
            resume()
        });
        assert_eq!(result.unwrap_err(), "scan:cancelled");
        assert!(file.exists().unwrap());
        let result = resume().unwrap();
        assert_eq!(result.diff.added_reg_values, ["after"]);
        assert!(result.diff.recovered_read_only);
        assert!(result.items.is_empty() && result.scope_revision.is_none());
        assert!(!crate::scan_allow::was_recent(
            crate::scan_allow::AllowScope::Monitor,
            "after"
        ));
        assert!(!file.exists().unwrap());
        begin_session(
            &restarted,
            capture,
            || file.exists(),
            |snapshot| file.save(snapshot),
        )
        .unwrap();
    }
}
