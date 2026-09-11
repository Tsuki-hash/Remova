//! Remova core library (Tauri backend).

pub mod apps;
pub mod regscan;
pub mod safety;
pub mod scanner;

use apps::InstalledApp;
use scanner::ScanResult;

#[tauri::command]
fn list_installed_apps() -> Result<Vec<InstalledApp>, String> {
    Ok(apps::scan_installed_apps())
}

#[tauri::command]
fn analyze_associations(app: InstalledApp) -> Result<ScanResult, String> {
    Ok(scanner::analyze_associations(
        &app.name,
        &app.install_location,
        &app.publisher,
        &app.registry_key,
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_installed_apps,
            analyze_associations
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
