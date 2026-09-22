//! Explorer context-menu 'deep uninstall' registration.

use crate::sysops;

#[tauri::command]
pub async fn register_context_menu() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(sysops::register_context_menu)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn unregister_context_menu() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(sysops::unregister_context_menu)
        .await
        .map_err(|e| e.to_string())?
}
