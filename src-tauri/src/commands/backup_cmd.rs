//! Safety Vault session listing, deletion and restore dispatch.

use crate::restore;

#[tauri::command]
pub fn list_restore_sessions() -> Result<Vec<String>, String> {
    Ok(restore::list_session_names())
}

#[tauri::command]
pub async fn list_backup_sessions() -> Result<Vec<restore::SessionInfo>, String> {
    tauri::async_runtime::spawn_blocking(restore::list_session_info)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_backup_session(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || restore::delete_session_by_name(&name))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn restore_session_by_name(name: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || restore::restore_by_name(&name))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    // Command-layer boundary coverage.

    struct IsolatedBackupRoot {
        _lock: std::sync::MutexGuard<'static, ()>,
        dir: std::path::PathBuf,
        previous: Option<std::ffi::OsString>,
    }

    impl IsolatedBackupRoot {
        fn new() -> Self {
            let lock = crate::backup::lock_backup_env();
            let dir = std::env::temp_dir().join(format!(
                "remova-backup-boundary-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            assert!(dir.is_absolute());
            std::fs::create_dir(&dir).unwrap();
            let previous = std::env::var_os("REMOVA_BACKUP_DIR");
            std::env::set_var("REMOVA_BACKUP_DIR", &dir);
            Self {
                _lock: lock,
                dir,
                previous,
            }
        }
    }

    impl Drop for IsolatedBackupRoot {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("REMOVA_BACKUP_DIR", previous);
            } else {
                std::env::remove_var("REMOVA_BACKUP_DIR");
            }
            if self.dir.parent() == Some(std::env::temp_dir().as_path()) {
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }
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
        assert!(!isolated.dir.join("20260922-120000_no_such").exists());
        match crate::restore::delete_session_by_name("20260922-120000_no_such") {
            Err(e) => assert!(e.contains("not found"), "got {e}"),
            Ok(()) => panic!("must not delete a missing session"),
        }
    }

    // -01: restore_by_name still rejects traversal at the command boundary.
    #[test]
    fn restore_session_by_name_rejects_traversal() {
        assert!(crate::restore::restore_by_name("").is_err());
        assert!(crate::restore::restore_by_name("..").is_err());
        assert!(crate::restore::restore_by_name("a/b").is_err());
        assert!(crate::restore::restore_by_name("a\\b").is_err());
    }
}
