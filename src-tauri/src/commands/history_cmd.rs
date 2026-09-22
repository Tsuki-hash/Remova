//! Cleanup history listing and CSV export.

use crate::fsutil;
use crate::history;
use crate::history::HistoryEntry;

#[tauri::command]
pub fn list_cleanup_history() -> Result<Vec<HistoryEntry>, String> {
    Ok(history::load(crate::constants::HISTORY_LIST_CAP))
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
