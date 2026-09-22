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
