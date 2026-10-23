//! Server-side allow-lists for scoped scans (installer / toolcache).
//! Same trust model as orphan paths : only paths from the latest scan may be deleted.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Per-app cleanup authorization from the latest analysis, bounded so a long
/// session cannot retain every app's paths forever.
type AppPaths = HashMap<String, (u64, HashSet<String>)>;
type AppMaps = Mutex<AppPaths>;
static APP_MAPS: OnceLock<AppMaps> = OnceLock::new();
static APP_GENERATION: AtomicU64 = AtomicU64::new(0);
const MAX_ANALYZED_APPS: usize = 64;

fn app_maps() -> &'static AppMaps {
    APP_MAPS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn item_kind_tag(kind: &crate::scanner::ItemKind) -> &'static str {
    match kind {
        crate::scanner::ItemKind::File => "file",
        crate::scanner::ItemKind::Dir => "dir",
        crate::scanner::ItemKind::Registry => "registry",
        crate::scanner::ItemKind::Path => "path",
    }
}

fn authorized_item_key(item: &crate::scanner::CleanupItem) -> Option<String> {
    normalize(&item.path).map(|key| {
        format!(
            "{key}\0{}\0{}",
            item_kind_tag(&item.kind),
            item.service_bound
        )
    })
}

/// Publish the cleanup items of one successful analysis as the only actions a
/// later uninstall cleanup may perform for that app. The scan identity, the
/// item kind and the service-bound flag all come from the backend, not from the
/// cleanup request.
pub fn remember_uninstall_items(app_key: &str, items: &[crate::scanner::CleanupItem]) {
    crate::scan_task::publish(|| {
        let normalized: HashSet<String> = items.iter().filter_map(authorized_item_key).collect();
        let generation = APP_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut g) = app_maps().lock() {
            g.insert(app_key.to_string(), (generation, normalized));
            if g.len() > MAX_ANALYZED_APPS {
                let mut keys: Vec<(u64, String)> =
                    g.iter().map(|(k, (seen, _))| (*seen, k.clone())).collect();
                keys.sort_unstable();
                for (_, k) in keys.into_iter().take(g.len() - MAX_ANALYZED_APPS) {
                    g.remove(&k);
                }
            }
        }
    });
}

/// True when an analysis for this exact app identity is still available.
pub fn uninstall_scan_recorded(app_key: &str) -> bool {
    app_maps()
        .lock()
        .map(|g| g.contains_key(app_key))
        .unwrap_or(false)
}

/// True only for the exact action (path, kind, service-bound flag) the latest
/// analysis of this app returned.
pub fn was_recent_uninstall_item(app_key: &str, item: &crate::scanner::CleanupItem) -> bool {
    let Some(key) = authorized_item_key(item) else {
        return false;
    };
    app_maps()
        .lock()
        .ok()
        .and_then(|g| g.get(app_key).map(|(_, items)| items.contains(&key)))
        .unwrap_or(false)
}

/// Comparison key for an allow-list entry.
///
/// Delegates to `fsutil::path_key`, the single definition of path identity. This
/// module used to carry its own copy, which was byte-identical to
/// `orphans::normalize_orphan_key` and — unlike `safety::path_utf8_lower` — did
/// not strip Win32's per-segment trailing dots and spaces. The result was
/// fail-closed by luck: a scan reporting `C:\Foo\Bar.` authorized the key
/// `c:\foo\bar.`, so the delete request for `C:\Foo\Bar` was refused and the user
/// was shown an item they could not remove. The red-line checks disagreed about
/// what the path even was. One implementation makes the agreement deliberate.
///
/// `None` for a path that cannot be named (non-UTF-8, or nothing left after
/// trimming) — such a path must not become an allow-list key.
fn normalize(path: &str) -> Option<String> {
    crate::fsutil::path_key(std::path::Path::new(path))
}

fn scoped_key(scope: AllowScope, path: &str) -> Option<String> {
    normalize(path).map(|key| format!("{}\0{}", scope.key(), key))
}

/// Replace the allow-list for `scope` with the latest scan results.
pub fn remember(scope: AllowScope, paths: &HashSet<String>) {
    crate::scan_task::publish(|| {
        if let Ok(mut g) = maps().lock() {
            g.retain(|k| !k.starts_with(&format!("{}\0", scope.key())));
            for p in paths {
                // A path that cannot be named is not authorized — skip it rather
                // than storing a key no delete request could ever match.
                if let Some(key) = scoped_key(scope, p) {
                    g.insert(key);
                }
            }
        }
    });
}

/// Publish the monitor scan's paths as the monitor allow-list.
///
/// Committing these items **authorizes deleting exactly those paths** for the
/// monitor scope: the scoped panels refuse anything the latest scan did not
/// report, so this is the moment a set of paths becomes deletable. It therefore
/// consumes the scan snapshot first and fails closed — if the snapshot cannot be
/// consumed the previous authorization is left in place and nothing new is
/// armed.
pub(crate) fn commit_monitor_items(
    _permit: &crate::scan_task::CommitPermit,
    paths: &HashSet<String>,
    consume_snapshot: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let mut g = maps().lock().map_err(|_| "scan:state")?;
    consume_snapshot()?;
    g.retain(|key| !key.starts_with("monitor\0"));
    for path in paths {
        if let Some(key) = scoped_key(AllowScope::Monitor, path) {
            g.insert(key);
        }
    }
    Ok(())
}

/// True when `path` was returned by the latest scan for `scope`.
///
/// The lookup is normalized the same way as the stored key, so case, slash style
/// and Win32's per-segment trailing dots/spaces of a reported path still
/// authorize; anything the latest scan did not report does not, including a path
/// from an older scan. A path that cannot be named never authorizes.
pub fn was_recent(scope: AllowScope, path: &str) -> bool {
    match scoped_key(scope, path) {
        Some(key) => maps().lock().map(|g| g.contains(&key)).unwrap_or(false),
        None => false,
    }
}

#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_snapshot_consumption_cannot_publish_monitor_authorization() {
        let _g = test_lock();
        remember(
            AllowScope::Monitor,
            &HashSet::from(["previous-monitor-path".into()]),
        );
        let result = crate::scan_task::commit(|permit| {
            commit_monitor_items(
                permit,
                &HashSet::from(["unpublished-monitor-path".into()]),
                || Err("fixture consume failed".into()),
            )
        });
        assert_eq!(result, Err("fixture consume failed".into()));
        assert!(was_recent(AllowScope::Monitor, "previous-monitor-path"));
        assert!(!was_recent(AllowScope::Monitor, "unpublished-monitor-path"));
    }

    #[test]
    fn cancelled_specialty_scans_preserve_previous_authorization() {
        let _g = test_lock();
        for (scope, scan) in [
            (
                AllowScope::Installer,
                crate::installers::scan_installer_caches
                    as fn() -> Result<crate::scanner::ScopedScanResult, String>,
            ),
            (
                AllowScope::ToolCache,
                crate::toolcache::scan_tool_caches
                    as fn() -> Result<crate::scanner::ScopedScanResult, String>,
            ),
        ] {
            let path = "cancelled-scan-previous-path";
            remember(scope, &HashSet::from([path.into()]));
            let id = crate::scan_task::begin().unwrap();
            assert_eq!(
                crate::scan_task::run(id, || {
                    crate::scan_task::cancel(id).unwrap();
                    let result = scan().expect("a cancelled scan still returns a result");
                    assert!(result.items.is_empty());
                    assert!(result.omitted == 0);
                }),
                Err("scan:cancelled".into())
            );
            assert!(was_recent(scope, path));
        }
    }

    #[test]
    fn scoped_allow_lists_are_isolated() {
        let _g = test_lock();
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
        let _g = test_lock();
        let mut a = HashSet::new();
        a.insert(r"C:\Users\x\AppData\Local\npm-cache".into());
        remember(AllowScope::ToolCache, &a);
        assert!(was_recent(
            AllowScope::ToolCache,
            r"c:/users/x/appdata/local/npm-cache/"
        ));
    }

    /// this allow-list now shares `fsutil::path_key` with the red-line
    /// checks, so the two halves of the gate must agree on the shapes Win32
    /// collapses. Before the delegation a reported `…\npm-cache.` stored the key
    /// `…\npm-cache.`, and the delete request for `…\npm-cache` was refused — the
    /// panel listed an item the user could not remove.
    #[test]
    fn normalize_matches_win32_on_trailing_dots_and_spaces() {
        let _g = test_lock();
        let mut a = HashSet::new();
        a.insert(r"C:\Users\x\AppData\Local\npm-cache.".into());
        remember(AllowScope::ToolCache, &a);
        assert!(was_recent(
            AllowScope::ToolCache,
            r"c:/users/x/appdata/local/npm-cache"
        ));
        // And the reverse direction: reported plain, requested decorated.
        let mut b = HashSet::new();
        b.insert(r"C:\Users\x\AppData\Local\pnpm-store".into());
        remember(AllowScope::ToolCache, &b);
        assert!(was_recent(
            AllowScope::ToolCache,
            r" C:\Users\x\AppData\Local\pnpm-store. "
        ));
    }

    /// A path that cannot be named must never become an authorization.
    #[test]
    fn unnameable_paths_are_never_authorized() {
        let _g = test_lock();
        let mut a = HashSet::new();
        a.insert("   ".into());
        a.insert("\"\"".into());
        remember(AllowScope::ToolCache, &a);
        assert!(!was_recent(AllowScope::ToolCache, "   "));
        assert!(!was_recent(AllowScope::ToolCache, r"C:\Anything"));
    }

    fn cleanup_item(path: &str) -> crate::scanner::CleanupItem {
        crate::scanner::CleanupItem {
            path: path.into(),
            kind: crate::scanner::ItemKind::Dir,
            score: 90,
            confidence: crate::scanner::Confidence::Confirmed,
            risk: crate::scanner::RiskLevel::Low,
            reason: "fixture".into(),
            evidence: vec![],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        }
    }

    #[test]
    fn uninstall_allow_list_is_per_app_and_normalized() {
        let _g = test_lock();
        let app = "HKLM64\0HKLM64\\SOFTWARE\\Demo\0Demo";
        remember_uninstall_items(app, &[cleanup_item(r"C:\Program Files\Demo")]);
        assert!(uninstall_scan_recorded(app));
        assert!(was_recent_uninstall_item(
            app,
            &cleanup_item(r"c:/program files/demo/")
        ));
        assert!(!was_recent_uninstall_item(
            app,
            &cleanup_item(r"C:\Program Files\Victim")
        ));
        assert!(!was_recent_uninstall_item(
            "HKCU\0HKCU\\SOFTWARE\\Demo\0Demo",
            &cleanup_item(r"C:\Program Files\Demo")
        ));
        assert!(!uninstall_scan_recorded("HKCU\0HKCU\\SOFTWARE\\Demo\0Demo"));
    }

    #[test]
    fn a_new_analysis_replaces_the_previous_app_allow_list() {
        let _g = test_lock();
        let app = "HKLM64\0HKLM64\\SOFTWARE\\Replace\0Replace";
        remember_uninstall_items(app, &[cleanup_item(r"C:\Old")]);
        remember_uninstall_items(app, &[cleanup_item(r"C:\New")]);
        assert!(!was_recent_uninstall_item(app, &cleanup_item(r"C:\Old")));
        assert!(was_recent_uninstall_item(app, &cleanup_item(r"C:\New")));
    }

    #[test]
    fn kind_and_service_bound_are_part_of_the_authorization() {
        let _g = test_lock();
        let app = "HKLM64\0HKLM64\\SOFTWARE\\Bound\0Bound";
        let mut item = cleanup_item(r"C:\Program Files\Bound");
        item.service_bound = true;
        remember_uninstall_items(app, &[item.clone()]);
        assert!(was_recent_uninstall_item(app, &item));
        let mut forged_kind = item.clone();
        forged_kind.kind = crate::scanner::ItemKind::Path;
        assert!(!was_recent_uninstall_item(app, &forged_kind));
        let mut forged_bound = item.clone();
        forged_bound.service_bound = false;
        assert!(!was_recent_uninstall_item(app, &forged_bound));
    }
}
