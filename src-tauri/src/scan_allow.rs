//! Server-side allow-lists for scoped scans (installer / toolcache).
//! Same trust model as orphan paths (S-R6-01): only paths from the latest scan may be deleted.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowScope {
    Installer,
    ToolCache,
    Monitor,
}

impl AllowScope {
    fn key(self) -> &'static str {
        match self {
            AllowScope::Installer => "installer",
            AllowScope::ToolCache => "toolcache",
            AllowScope::Monitor => "monitor",
        }
    }
}

static MAPS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn maps() -> &'static Mutex<HashSet<String>> {
    MAPS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn normalize(path: &str) -> String {
    path.replace('/', "\\")
        .trim()
        .trim_matches('"')
        .trim_end_matches('\\')
        .to_lowercase()
}

fn scoped_key(scope: AllowScope, path: &str) -> String {
    format!("{}\0{}", scope.key(), normalize(path))
}

/// Replace the allow-list for `scope` with the latest scan results.
pub fn remember(scope: AllowScope, paths: &HashSet<String>) {
    if let Ok(mut g) = maps().lock() {
        g.retain(|k| !k.starts_with(&format!("{}\0", scope.key())));
        for p in paths {
            g.insert(scoped_key(scope, p));
        }
    }
}

/// True when `path` was returned by the latest scan for `scope`.
pub fn was_recent(scope: AllowScope, path: &str) -> bool {
    maps()
        .lock()
        .map(|g| g.contains(&scoped_key(scope, path)))
        .unwrap_or(false)
}

#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// These tests mutate the process-wide allow-list — serialize them.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn scoped_allow_lists_are_isolated() {
        let _g = TEST_LOCK.lock().unwrap();
        let mut a = HashSet::new();
        a.insert(r"C:\Users\x\Downloads\app.msi".into());
        remember(AllowScope::Installer, &a);
        assert!(was_recent(
            AllowScope::Installer,
            r"C:\Users\x\Downloads\app.msi"
        ));
        assert!(!was_recent(
            AllowScope::ToolCache,
            r"C:\Users\x\Downloads\app.msi"
        ));
    }

    #[test]
    fn normalize_repels_case_and_slash_variants() {
        let _g = TEST_LOCK.lock().unwrap();
        let mut a = HashSet::new();
        a.insert(r"C:\Users\x\AppData\Local\npm-cache".into());
        remember(AllowScope::ToolCache, &a);
        assert!(was_recent(
            AllowScope::ToolCache,
            r"c:/users/x/appdata/local/npm-cache/"
        ));
    }
}
