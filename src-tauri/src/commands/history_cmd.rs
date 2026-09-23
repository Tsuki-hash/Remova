//! Cleanup history listing, CSV export, and deletion.

use crate::fsutil;
use crate::history;
use crate::history::HistoryEntry;

#[tauri::command]
pub fn list_cleanup_history() -> Result<Vec<HistoryEntry>, String> {
    Ok(history::load(crate::constants::HISTORY_LIST_CAP))
}

/// Delete rows by runtime ids from `list_cleanup_history`. Returns removed count.
#[tauri::command]
pub async fn delete_cleanup_history(ids: Vec<String>) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || history::delete_by_ids(&ids))
        .await
        .map_err(|e| e.to_string())?
}

/// Clear the cleanup history log (backup sessions are left untouched).
#[tauri::command]
pub async fn clear_cleanup_history() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(history::clear_all)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn export_history_csv() -> Result<String, String> {
    let entries = history::load(crate::constants::HISTORY_CSV_CAP);
    let mut out =
        String::from("app_name,deleted,failed,skipped,delayed,aborted,backup_dir,created_at\n");
    for e in entries {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{}\n",
            fsutil::csv_escape(&e.app_name),
            e.deleted,
            e.failed,
            e.skipped,
            e.delayed,
            e.aborted,
            fsutil::csv_escape(&e.backup_dir),
            e.created_at
        ));
    }
    Ok(out)
}
