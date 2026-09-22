//! Ignore-list rules: read, mutate, suggest, apply.

use crate::ignore;

#[tauri::command]
pub fn load_ignore() -> Result<ignore::IgnoreList, String> {
    Ok(ignore::load())
}

#[tauri::command]
pub fn ignore_publisher(name: String) -> Result<ignore::IgnoreList, String> {
    ignore::add_publisher(&name)
}

#[tauri::command]
pub fn ignore_app_name(name: String) -> Result<ignore::IgnoreList, String> {
    ignore::add_name(&name)
}

/// Rule-based ignore suggestions from leftover paths (shared runtimes).
#[tauri::command]
pub fn suggest_ignore_rules(
    publisher: String,
    paths: Vec<String>,
) -> Result<Vec<ignore::IgnoreSuggestion>, String> {
    Ok(ignore::suggest_from_leftovers(&publisher, &paths))
}

#[tauri::command]
pub fn apply_ignore_suggestions(
    suggestions: Vec<ignore::IgnoreSuggestion>,
) -> Result<ignore::IgnoreList, String> {
    ignore::apply_suggestions(&suggestions)
}
