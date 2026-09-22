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
}
