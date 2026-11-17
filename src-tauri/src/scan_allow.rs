//! Server-side cleanup authorization from the latest scans.

use crate::scanner::{CleanupItem, ItemKind};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AllowScope {
    Installer,
    ToolCache,
    Monitor,
}

#[derive(Clone)]
struct ScopedItem {
    item: CleanupItem,
    file_id: Option<(u32, u64)>,
}

type ScopedItems = HashMap<String, ScopedItem>;
type ScopedMaps = Mutex<HashMap<AllowScope, (u64, ScopedItems)>>;
static MAPS: OnceLock<ScopedMaps> = OnceLock::new();
static SCOPED_GENERATION: AtomicU64 = AtomicU64::new(0);

fn maps() -> &'static ScopedMaps {
    MAPS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Per-app cleanup authorization from the latest analysis, bounded so a long
/// session cannot retain every app's paths forever.
type FileId = (u32, u64);
type AppPaths = HashMap<String, (u64, HashMap<String, Option<FileId>>)>;
type AppMaps = Mutex<AppPaths>;
static APP_MAPS: OnceLock<AppMaps> = OnceLock::new();
static APP_GENERATION: AtomicU64 = AtomicU64::new(0);
const MAX_ANALYZED_APPS: usize = 64;
pub(crate) const ORPHAN_SCAN_KEY: &str = "orphan-scan";
pub(crate) const APP_OBJECT_CHANGED: &str = "cleanup object no longer matches the scan; scan again";

/// Selected objects from a single server-side analysis. Hold it through the
/// operation so another scan cannot replace the identities mid-cleanup.
pub(crate) struct AppFileSnapshot(HashMap<String, FileId>);

impl AppFileSnapshot {
    pub(crate) fn expected(&self, item: &CleanupItem) -> Option<FileId> {
        authorized_item_key(item).and_then(|key| self.0.get(&key).copied())
    }

    pub(crate) fn matches(&self, item: &CleanupItem) -> bool {
        !matches!(item.kind, ItemKind::File | ItemKind::Dir)
            || self
                .expected(item)
                .is_some_and(|id| file_identity(item).ok() == Some(id))
    }
}

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

pub(crate) fn authorized_item_key(item: &crate::scanner::CleanupItem) -> Option<String> {
    normalize(&item.path).map(|key| item_key(&key, item))
}

fn item_key(path: &str, item: &CleanupItem) -> String {
    // Security-relevant classification is part of the authorization
    // snapshot. A renderer may not clear shared/service/user-data flags
    // while keeping the same path and kind.
    format!(
        "{path}\0{}\0{}\0{}\0{}\0{}",
        item_kind_tag(&item.kind),
        item.shared,
        item.service_bound,
        item.user_data,
        item.user_library
    )
}

/// Publish the cleanup items of one successful analysis as the only actions a
/// later uninstall cleanup may perform for that app. The scan identity, the
/// item kind and the service-bound flag all come from the backend, not from the
/// cleanup request.
pub fn remember_uninstall_items(app_key: &str, items: &[CleanupItem]) -> Option<u64> {
    let normalized = items
        .iter()
        .take_while(|_| !crate::scan_task::cancelled())
        .filter_map(|item| {
            let key = authorized_item_key(item)?;
            let identity = match item.kind {
                ItemKind::File | ItemKind::Dir => file_identity(item).ok(),
                ItemKind::Registry | ItemKind::Path => None,
            };
            Some((key, identity))
        })
        .collect();
    let mut revision = None;
    crate::scan_task::publish(|| {
        let generation = APP_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut g) = app_maps().lock() {
            g.insert(app_key.to_string(), (generation, normalized));
            revision = Some(generation);
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
    revision
}

#[cfg(test)]
pub fn uninstall_scan_recorded(app_key: &str) -> bool {
    app_maps()
        .lock()
        .map(|g| g.contains_key(app_key))
        .unwrap_or(false)
}

#[cfg(test)]
pub fn was_recent_uninstall_item(app_key: &str, item: &crate::scanner::CleanupItem) -> bool {
    resolve_app_files(app_key, app_revision(app_key), std::slice::from_ref(item)).is_ok()
}

pub(crate) fn app_item_was_reported(app_key: &str, item: &CleanupItem) -> bool {
    authorized_item_key(item).is_some_and(|key| {
        app_maps().lock().is_ok_and(|maps| {
            maps.get(app_key)
                .is_some_and(|(_, items)| items.contains_key(&key))
        })
    })
}

#[cfg(test)]
pub(crate) fn app_path_was_reported(app_key: &str, path: &str) -> bool {
    normalize(path).is_some_and(|path| {
        let prefix = format!("{path}\0");
        app_maps().lock().is_ok_and(|maps| {
            maps.get(app_key)
                .is_some_and(|(_, items)| items.keys().any(|key| key.starts_with(&prefix)))
        })
    })
}

#[cfg(test)]
pub(crate) fn app_revision(app_key: &str) -> Option<u64> {
    app_maps()
        .lock()
        .ok()?
        .get(app_key)
        .map(|(revision, _)| *revision)
}

pub(crate) fn resolve_app_files(
    app_key: &str,
    revision: Option<u64>,
    items: &[CleanupItem],
) -> Result<AppFileSnapshot, String> {
    let maps = app_maps().lock().map_err(|_| "scan:state")?;
    let (_, originals) = maps
        .get(app_key)
        .filter(|(current, _)| Some(*current) == revision)
        .ok_or("analysis must run before cleanup")?;
    let mut selected = HashMap::new();
    for item in items.iter().filter(|item| !item.path.trim().is_empty()) {
        let Some((key, identity)) = authorized_item_key(item)
            .and_then(|key| originals.get(&key).map(|identity| (key, *identity)))
        else {
            return Err(format!(
                "cleanup item not from the latest analysis: {}",
                item.path
            ));
        };
        if let Some(id) = identity {
            selected.insert(key, id);
        }
    }
    Ok(AppFileSnapshot(selected))
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

fn file_identity(item: &CleanupItem) -> std::io::Result<(u32, u64)> {
    #[cfg(windows)]
    {
        let pins = crate::fsutil::pin_local_scan_path(Path::new(&item.path))?;
        let object = pins
            .last()
            .ok_or_else(|| std::io::Error::other("empty path"))?;
        let metadata = crate::fsutil::pin_metadata(object)?;
        if !(metadata.is_file() && item.kind == ItemKind::File
            || metadata.is_dir() && item.kind == ItemKind::Dir)
        {
            return Err(std::io::Error::other("cleanup object type changed"));
        }
        object.file_identity()
    }
    #[cfg(not(windows))]
    {
        let _ = item;
        Err(std::io::Error::other(
            "cleanup object identity requires Windows",
        ))
    }
}

fn scope_items(items: &[CleanupItem]) -> ScopedItems {
    items
        .iter()
        .filter_map(|item| {
            normalize(&item.path)?;
            let file_id = match item.kind {
                ItemKind::File | ItemKind::Dir => Some(file_identity(item).ok()?),
                ItemKind::Registry => None,
                ItemKind::Path => return None, // These scanners never authorize a PATH edit.
            };
            Some((
                item_key(&item.path, item),
                ScopedItem {
                    item: item.clone(),
                    file_id,
                },
            ))
        })
        .collect()
}

pub fn remember(scope: AllowScope, items: &mut Vec<CleanupItem>) -> Option<u64> {
    let originals = scope_items(items);
    items.retain(|item| originals.contains_key(&item_key(&item.path, item)));
    let mut generation = None;
    crate::scan_task::publish(|| {
        if let Ok(mut g) = maps().lock() {
            let revision = SCOPED_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
            g.insert(scope, (revision, originals));
            generation = Some(revision);
        }
    });
    generation
}

/// Publish the monitor authorization only after the matching snapshot update.
pub(crate) fn commit_monitor_items(
    _permit: &crate::scan_task::CommitPermit,
    items: &[CleanupItem],
    update_snapshot: impl FnOnce() -> Result<(), String>,
) -> Result<u64, String> {
    let items = scope_items(items);
    update_snapshot()?;
    // The caller owns its session and commit permit through persistence. No
    // global authorization lock is held during serialization or disk I/O.
    let mut g = maps().lock().unwrap_or_else(|error| error.into_inner());
    let revision = SCOPED_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    g.insert(AllowScope::Monitor, (revision, items));
    Ok(revision)
}

pub(crate) fn scoped_item_was_reported(scope: AllowScope, item: &CleanupItem) -> bool {
    let key = item_key(&item.path, item);
    maps()
        .lock()
        .ok()
        .and_then(|g| g.get(&scope).map(|(_, items)| items.contains_key(&key)))
        .unwrap_or(false)
}

pub(crate) fn resolve_scoped_items(
    scope: AllowScope,
    revision: Option<u64>,
    items: &[CleanupItem],
) -> Result<Vec<CleanupItem>, String> {
    let entries = {
        let g = maps().lock().map_err(|_| "scan:state")?;
        let (_, originals) = g
            .get(&scope)
            .filter(|(current, _)| Some(*current) == revision)
            .ok_or("specialty cleanup scan expired; scan again")?;
        items
            .iter()
            .map(|item| {
                originals
                    .get(&item_key(&item.path, item))
                    .cloned()
                    .ok_or_else(|| format!("specialty cleanup item not authorized: {}", item.path))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    entries
        .into_iter()
        .map(|entry| {
            if entry.file_id.is_some() && file_identity(&entry.item).ok() != entry.file_id {
                return Err(format!(
                    "specialty cleanup object changed: {}",
                    entry.item.path
                ));
            }
            Ok(entry.item)
        })
        .collect()
}

pub(crate) fn scoped_file_id(
    scope: AllowScope,
    revision: Option<u64>,
    path: &Path,
) -> Result<(u32, u64), String> {
    let g = maps().lock().map_err(|_| "scan:state")?;
    let (_, items) = g
        .get(&scope)
        .filter(|(current, _)| Some(*current) == revision)
        .ok_or("specialty cleanup scan expired; scan again")?;
    items
        .values()
        .find(|entry| Path::new(&entry.item.path) == path)
        .and_then(|entry| entry.file_id)
        .ok_or_else(|| format!("specialty cleanup object changed: {}", path.display()))
}

#[cfg(test)]
pub(crate) fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: Mutex<()> = Mutex::new(());
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub(crate) use tests::cleanup_item as test_item;

#[cfg(test)]
pub(crate) fn remember_for_test(scope: AllowScope, items: &[CleanupItem]) {
    let originals = items
        .iter()
        .map(|item| {
            (
                item_key(&item.path, item),
                ScopedItem {
                    item: item.clone(),
                    file_id: None,
                },
            )
        })
        .collect();
    let revision = SCOPED_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    maps().lock().unwrap().insert(scope, (revision, originals));
}

#[cfg(test)]
pub(crate) fn was_recent(scope: AllowScope, path: &str) -> bool {
    maps()
        .lock()
        .unwrap()
        .get(&scope)
        .is_some_and(|(_, items)| items.values().any(|entry| entry.item.path == path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_snapshot_consumption_cannot_publish_monitor_authorization() {
        let _g = test_lock();
        remember_for_test(
            AllowScope::Monitor,
            &[cleanup_item("previous-monitor-path")],
        );
        let result = crate::scan_task::commit(|permit| {
            commit_monitor_items(permit, &[cleanup_item("unpublished-monitor-path")], || {
                Err("fixture consume failed".into())
            })
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
            remember_for_test(scope, &[cleanup_item(path)]);
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

    pub(crate) fn cleanup_item(path: &str) -> crate::scanner::CleanupItem {
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

    #[cfg(windows)]
    #[test]
    fn scoped_cleanup_requires_the_same_action_scope_revision_and_native_object() {
        let _guard = test_lock();
        let fixture = crate::test_support::TempDir::new("scope-identity");
        let original = fixture.subdir("fixture-cache");
        let mut item = cleanup_item(original.to_str().unwrap());
        item.user_library = true;
        let scope = AllowScope::Monitor;
        let revision = remember(scope, &mut vec![item.clone()]);
        assert!(revision.is_some());

        let check = |scope, revision, candidate: &CleanupItem| {
            resolve_scoped_items(scope, revision, std::slice::from_ref(candidate))
        };
        let mut edited = item.clone();
        edited.reason = "client reason".into();
        assert_eq!(
            check(scope, revision, &edited).unwrap()[0].reason,
            item.reason
        );
        assert!(check(AllowScope::ToolCache, revision, &item).is_err());
        assert!(check(scope, None, &item).is_err());
        edited.kind = ItemKind::Path;
        assert!(check(scope, revision, &edited).is_err());
        edited = item.clone();
        edited.user_library = false;
        assert!(check(scope, revision, &edited).is_err());
        edited = item.clone();
        edited.path.push('\\');
        assert!(check(scope, revision, &edited).is_err());

        let newer = remember(scope, &mut vec![item.clone()]);
        assert!(check(scope, revision, &item).is_err());
        let expected = scoped_file_id(scope, newer, &original).unwrap();
        std::fs::rename(&original, fixture.join("original")).unwrap();
        std::fs::create_dir(&original).unwrap();
        let marker = original.join("keep.txt");
        std::fs::write(&marker, b"keep").unwrap();
        assert!(check(scope, newer, &item).is_err());
        assert!(crate::fsutil::remove_scanned_tree(&original, expected).is_err());
        assert_eq!(std::fs::read(marker).unwrap(), b"keep");
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

    #[test]
    fn all_security_classification_flags_are_bound_to_the_scan() {
        let _g = test_lock();
        let app = "HKLM64\0HKLM64\\SOFTWARE\\Flags\0Flags";
        let mut item = cleanup_item(r"C:\Program Files\Flags");
        item.shared = true;
        item.user_data = true;
        item.user_library = true;
        remember_uninstall_items(app, &[item.clone()]);
        assert!(was_recent_uninstall_item(app, &item));
        for mutate in [
            |it: &mut crate::scanner::CleanupItem| it.shared = false,
            |it: &mut crate::scanner::CleanupItem| it.user_data = false,
            |it: &mut crate::scanner::CleanupItem| it.user_library = false,
        ] {
            let mut forged = item.clone();
            mutate(&mut forged);
            assert!(!was_recent_uninstall_item(app, &forged));
        }
    }
}
