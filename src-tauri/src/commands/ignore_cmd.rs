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

#[tauri::command]
pub fn unignore_publisher(name: String) -> Result<ignore::IgnoreList, String> {
    ignore::remove_publisher(&name)
}

#[tauri::command]
pub fn unignore_app_name(name: String) -> Result<ignore::IgnoreList, String> {
    ignore::remove_name(&name)
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

#[cfg(test)]
mod tests {
    // R-R6-07: command-layer boundary coverage (was zero).

    #[test]
    fn ignore_publisher_rejects_blank_name() {
        // Empty / whitespace publisher must never become an ignore rule.
        assert!(super::ignore_publisher("   ".to_string()).is_err());
        assert!(super::ignore_publisher("".to_string()).is_err());
    }

    #[test]
    fn ignore_app_name_rejects_blank_name() {
        assert!(super::ignore_app_name("".to_string()).is_err());
        assert!(super::ignore_app_name("\t".to_string()).is_err());
    }
}
