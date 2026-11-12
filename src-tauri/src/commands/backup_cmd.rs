//! Safety Vault session listing, deletion and restore dispatch.

use crate::restore;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

struct ActiveRestore {
    id: String,
    cancelled: Arc<AtomicBool>,
}

static ACTIVE_RESTORE: Mutex<Option<ActiveRestore>> = Mutex::new(None);

struct RestoreOperation(Arc<AtomicBool>);

impl RestoreOperation {
    fn register(id: String) -> Result<Self, String> {
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("invalid restore operation".into());
        }
        let mut active = ACTIVE_RESTORE.lock().unwrap_or_else(|e| e.into_inner());
        if active.is_some() {
            return Err("a restore operation is already active".into());
        }
        let flag = Arc::new(AtomicBool::new(false));
        *active = Some(ActiveRestore {
            id,
            cancelled: Arc::clone(&flag),
        });
        Ok(Self(flag))
    }
}

impl Drop for RestoreOperation {
    fn drop(&mut self) {
        *ACTIVE_RESTORE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

#[tauri::command]
pub fn list_restore_sessions() -> Result<Vec<String>, String> {
    restore::list_session_names()
}

#[tauri::command]
pub async fn list_backup_sessions() -> Result<Vec<restore::SessionInfo>, String> {
    tauri::async_runtime::spawn_blocking(restore::list_session_info)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_backup_session(
    name: String,
    expected_status: Option<restore::SessionStatus>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        restore::delete_session_with_status(&name, expected_status)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn restore_session_by_name(
    name: String,
    operation_id: Option<String>,
) -> Result<Vec<String>, String> {
    let operation = operation_id.map(RestoreOperation::register).transpose()?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(operation) = operation {
            crate::executor::with_operation_cancellation(Arc::clone(&operation.0), || {
                restore::restore_by_name(&name)
            })
        } else {
            restore::restore_by_name(&name)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn cancel_restore(operation_id: String) -> bool {
    let active = ACTIVE_RESTORE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(restore) = active.as_ref().filter(|restore| restore.id == operation_id) {
        restore.cancelled.store(true, Ordering::Release);
        return true;
    }
    false
}

#[tauri::command]
pub async fn preview_restore_session(name: String) -> Result<restore::RestorePreview, String> {
    tauri::async_runtime::spawn_blocking(move || restore::preview_by_name(&name))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    // Command-layer boundary coverage.

    #[test]
    fn restore_cancellation_belongs_to_one_request_and_does_not_cancel_cleanup() {
        let operation = super::RestoreOperation::register("first".into()).unwrap();
        assert!(!super::cancel_restore("unrelated".into()));
        assert!(super::RestoreOperation::register("second".into()).is_err());
        crate::executor::begin_cleanup_control();
        assert!(super::cancel_restore("first".into()));
        assert!(!crate::executor::cleanup_cancelled());
        crate::executor::with_operation_cancellation(std::sync::Arc::clone(&operation.0), || {
            assert!(crate::executor::cleanup_cancelled());
        });
        assert!(!crate::executor::cleanup_cancelled());
        drop(operation);
        assert!(!super::cancel_restore("first".into()));
        let second = super::RestoreOperation::register("second".into()).unwrap();
        crate::executor::with_operation_cancellation(std::sync::Arc::clone(&second.0), || {
            assert!(!crate::executor::cleanup_cancelled());
        });
    }

    /// Redirects the backup root into a fixture directory for the guard's lifetime.
    ///
    /// This was the crate's only correctly-written fixture — RAII on both the
    /// environment and the directory, with the lock held for the whole scope — and
    /// is now built from `test_support`, which is where the other five copies were
    /// migrated to.
    ///
    /// **Field order is load-bearing.** Members drop in declaration order, so the
    /// lock must be declared LAST: it has to outlive the `EnvVar` whose `Drop`
    /// writes the environment. With the lock first, it was released before the
    /// override was restored, and a test that grabbed the lock in that window had
    /// its override deleted out from under it — `session_list_distinguishes_...`
    /// then read `REMOVA_BACKUP_DIR` as unset and fell through to the real
    /// `%ProgramData%\Remova\Backup`, which on a developer machine has real
    /// sessions in it. That is a flake that only appears on a machine that has
    /// actually used the product, which is the worst kind.
    struct IsolatedBackupRoot {
        _env: crate::test_support::EnvVar,
        _dir: crate::test_support::TempDir,
        _lock: crate::test_support::EnvLock,
    }

    impl IsolatedBackupRoot {
        fn new() -> Self {
            let lock = crate::test_support::EnvLock::new();
            let dir = crate::test_support::TempDir::new("backup-boundary");
            assert!(dir.is_absolute());
            let env = crate::test_support::EnvVar::set("REMOVA_BACKUP_DIR", &*dir);
            Self {
                _env: env,
                _dir: dir,
                _lock: lock,
            }
        }

        /// The fixture directory the backup root currently points at.
        fn path(&self) -> &std::path::Path {
            &self._dir
        }
    }

    #[test]
    fn session_list_distinguishes_missing_empty_and_read_failure() {
        let isolated = IsolatedBackupRoot::new();
        assert!(super::list_restore_sessions().unwrap().is_empty());
        std::fs::remove_dir(isolated.path()).unwrap();
        assert!(super::list_restore_sessions().unwrap().is_empty());
        std::fs::write(isolated.path(), b"not a directory").unwrap();
        assert!(super::list_restore_sessions().is_err());
        assert!(crate::restore::list_session_info().is_err());
        std::fs::remove_file(isolated.path()).unwrap();
        std::fs::create_dir(isolated.path()).unwrap();
        // list_restore_sessions filters sealed sessions, so the fixture
        // needs a real seal to be offered for restore. The seals root lives
        // OUTSIDE the backup root — list_session_info reports every directory
        // inside the backup root as a session.
        let seal_dir = crate::test_support::TempDir::new("backup-seals");
        let _g = crate::path_seal::test_lock();
        crate::path_seal::set_seals_root_for_tests(Some(seal_dir.path().to_path_buf()));
        let sess = isolated.path().join("20261003-120000_test");
        std::fs::create_dir(&sess).unwrap();
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            "abc_file.txt".to_string(),
            r"C:\Nonexistent\file.txt".to_string(),
        );
        std::fs::create_dir(sess.join("files")).unwrap();
        std::fs::write(
            sess.join("files/path_map.json"),
            serde_json::to_vec(&map).unwrap(),
        )
        .unwrap();
        std::fs::write(sess.join("files/abc_file.txt"), b"legacy backup fixture").unwrap();
        crate::path_seal::write_seal(&sess, "", &map, &std::collections::BTreeMap::new()).unwrap();
        // List under the same override — the seal only verifies against this root.
        assert_eq!(
            super::list_restore_sessions().unwrap(),
            ["20261003-120000_test"]
        );
        assert_eq!(crate::restore::list_session_info().unwrap().len(), 1);
        crate::path_seal::set_seals_root_for_tests(None);
    }

    #[test]
    fn delete_backup_session_rejects_dot_and_empty() {
        // / command boundary: `"."` / empty must never wipe the backup root.
        assert!(crate::restore::delete_session_by_name("").is_err());
        assert!(crate::restore::delete_session_by_name(".").is_err());
        assert!(crate::restore::delete_session_by_name("..").is_err());
        assert!(crate::restore::delete_session_by_name("a/b").is_err());
        assert!(crate::restore::delete_session_by_name("a\\b").is_err());
    }

    // Command-layer coverage for session-name shape.
    #[test]
    fn delete_backup_session_requires_timestamp_prefix() {
        let isolated = IsolatedBackupRoot::new();
        // Non-session folder names must never be deleted by name.
        assert!(crate::restore::delete_session_by_name("not-a-session").is_err());
        assert!(crate::restore::delete_session_by_name("2026-09-22").is_err());
        assert!(crate::restore::delete_session_by_name("backup").is_err());
        assert!(crate::restore::delete_session_by_name("20260922-abc").is_err());
        // Well-shaped missing sessions report not-found (name accepted, dir absent).
        assert!(!isolated.path().join("20260922-120000_no_such").exists());
        match crate::restore::delete_session_by_name("20260922-120000_no_such") {
            Err(e) => assert!(e.contains("not found"), "got {e}"),
            Ok(()) => panic!("must not delete a missing session"),
        }
    }

    // restore_by_name still rejects traversal at the command boundary.
    #[test]
    fn restore_session_by_name_rejects_traversal() {
        assert!(crate::restore::restore_by_name("").is_err());
        assert!(crate::restore::restore_by_name("..").is_err());
        assert!(crate::restore::restore_by_name("a/b").is_err());
        assert!(crate::restore::restore_by_name("a\\b").is_err());
    }
}
