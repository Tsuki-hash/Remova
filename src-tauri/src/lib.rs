//! Remova core library (Tauri backend).

pub mod apps;
pub mod backup;
pub mod executor;
pub mod history;
pub mod regops;
pub mod regscan;
pub mod restore;
pub mod safety;
pub mod scanner;

use apps::InstalledApp;
use executor::{CleanupReport, FullCleanupOptions, FullCleanupReport};
use history::HistoryEntry;
use scanner::{CleanupItem, ScanResult};

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

#[tauri::command]
fn run_cleanup_dry_run(app_name: String, items: Vec<CleanupItem>) -> Result<CleanupReport, String> {
    Ok(executor::run_cleanup_dry(&app_name, &items))
}

#[tauri::command]
fn run_full_cleanup(
    app: InstalledApp,
    items: Vec<CleanupItem>,
    options: FullCleanupOptions,
) -> Result<FullCleanupReport, String> {
    let report = executor::run_full_cleanup(&app, &items, &options);
    history::append(
        &report.app_name,
        report.deleted,
        report.failed,
        report.skipped,
        report.aborted,
        report.dry_run,
        &report.backup_dir,
    );
    Ok(report)
}

#[tauri::command]
fn restore_latest_backup() -> Result<Vec<String>, String> {
    let sessions = restore::list_sessions();
    let Some(s) = sessions.first() else {
        return Err("no backup sessions".into());
    };
    restore::restore_session(s)
}

#[tauri::command]
fn list_cleanup_history() -> Result<Vec<HistoryEntry>, String> {
    Ok(history::load(200))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_installed_apps,
            analyze_associations,
            run_cleanup_dry_run,
            run_full_cleanup,
            restore_latest_backup,
            list_cleanup_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
