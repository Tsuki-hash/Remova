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
    // R-R6-07: command-layer boundary coverage.

    #[test]
    fn delete_backup_session_rejects_dot_and_empty() {
        // S-R6-04 / command boundary: `"."` / empty must never wipe the backup root.
        assert!(crate::restore::delete_session_by_name("").is_err());
        assert!(crate::restore::delete_session_by_name(".").is_err());
        assert!(crate::restore::delete_session_by_name("..").is_err());
        assert!(crate::restore::delete_session_by_name("a/b").is_err());
        assert!(crate::restore::delete_session_by_name("a\\b").is_err());
    }

    // T-R7-01: commands-layer coverage for session-name shape (R-R7-02).
    #[test]
    fn delete_backup_session_requires_timestamp_prefix() {
        // Non-session folder names must never be deleted by name.
        assert!(crate::restore::delete_session_by_name("not-a-session").is_err());
        assert!(crate::restore::delete_session_by_name("2026-09-22").is_err());
        assert!(crate::restore::delete_session_by_name("backup").is_err());
        assert!(crate::restore::delete_session_by_name("20260922-abc").is_err());
        // Well-shaped missing sessions report not-found (name accepted, dir absent).
        match crate::restore::delete_session_by_name("20260922-120000_no_such") {
            Err(e) => assert!(e.contains("not found"), "got {e}"),
            Ok(()) => panic!("must not delete a missing session"),
        }
    }

    // T-R7-01: restore_by_name still rejects traversal at the command boundary.
    #[test]
    fn restore_session_by_name_rejects_traversal() {
        assert!(crate::restore::restore_by_name("").is_err());
        assert!(crate::restore::restore_by_name("..").is_err());
        assert!(crate::restore::restore_by_name("a/b").is_err());
        assert!(crate::restore::restore_by_name("a\\b").is_err());
    }
}
