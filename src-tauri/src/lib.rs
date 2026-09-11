//! Remova core library (Tauri backend).

pub mod apps;
pub mod safety;

use apps::InstalledApp;

#[tauri::command]
fn list_installed_apps() -> Result<Vec<InstalledApp>, String> {
    Ok(apps::scan_installed_apps())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![list_installed_apps])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
