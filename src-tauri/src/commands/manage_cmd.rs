//! Startup / service / scheduled-task management commands.

use crate::manage;

#[tauri::command]
pub async fn list_startup_items() -> Result<Vec<manage::ManageItem>, String> {
    tauri::async_runtime::spawn_blocking(manage::list_startup_items)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_services() -> Result<Vec<manage::ManageItem>, String> {
    tauri::async_runtime::spawn_blocking(manage::list_services)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_scheduled_tasks() -> Result<Vec<manage::ManageItem>, String> {
    tauri::async_runtime::spawn_blocking(manage::list_scheduled_tasks)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_startup_enabled(location: String, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || manage::set_startup_enabled(&location, enabled))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_service_start_disabled(name: String, disable: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || manage::set_service_start_disabled(&name, disable))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_service_running(name: String, run: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || manage::set_service_running(&name, run))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_task_enabled(name: String, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || manage::set_task_enabled(&name, enabled))
        .await
        .map_err(|e| e.to_string())?
}
