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

#[cfg(test)]
mod tests {
    // T-R7-01: commands-layer boundary coverage for manage write gates.

    #[test]
    fn set_service_start_disabled_rejects_critical_and_bad_names() {
        // Critical services must never be disabled via the manage command path.
        assert!(crate::manage::set_service_start_disabled("WinDefend", true).is_err());
        assert!(crate::manage::set_service_start_disabled("EventLog", true).is_err());
        assert!(crate::manage::set_service_start_disabled("", true).is_err());
        assert!(crate::manage::set_service_start_disabled("a\\b", true).is_err());
    }

    #[test]
    fn set_startup_enabled_rejects_blank_location() {
        assert!(crate::manage::set_startup_enabled("", true).is_err());
        assert!(crate::manage::set_startup_enabled("   ", false).is_err());
    }
}
