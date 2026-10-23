//! Explicit install-monitor snapshots (-1). User starts/stops; no driver.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FsSnapshot {
    pub files: BTreeSet<String>,
    /// True when the walk hit the budget or depth cap — the file set is
    /// incomplete, so end() must never arm the allow-list from it.
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorDiff {
    pub added_files: Vec<String>,
    pub added_reg_values: Vec<String>,
    /// entries beyond the diff caps — honest truncation, not silence.
    #[serde(default)]
    pub files_truncated: usize,
    #[serde(default)]
    pub reg_truncated: usize,
    /// A walk hit its budget or depth cap: the diff may miss entries that
    /// never changed (they were armed nowhere — see `end`). Consumers should
    /// surface this instead of presenting the diff as complete.
    #[serde(default)]
    pub walk_degraded: bool,
}

/// Server-side end payload: the diff the user sees plus cleanup items armed only from that diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorEndResult {
    pub diff: MonitorDiff,
    pub items: Vec<crate::scanner::CleanupItem>,
}

// tests must never touch the real PROGRAMDATA snapshot — the override
// is compile-time test-only and every test clears it when done.
#[cfg(test)]
static TEST_STATE_PATH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn set_test_state_path(p: Option<PathBuf>) {
    *TEST_STATE_PATH.lock().unwrap_or_else(|e| e.into_inner()) = p;
}

fn monitor_state_path() -> PathBuf {
    #[cfg(test)]
    if let Some(p) = TEST_STATE_PATH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        return p;
    }
    #[cfg(windows)]
    let base = crate::sysops::known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramData)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    #[cfg(not(windows))]
    let base = std::env::var("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"));
    base.join("Remova").join("monitor_snapshot.json")
}

/// Roots the install monitor snapshots.
///
/// `%APPDATA%` and `%ProgramData%` belong here: an installer that writes only
/// there used to produce an empty diff while the UI reported a completed scan.
/// The rest of the product already walks both (`fs_scans`), which is what proves
/// this list was incomplete rather than deliberately narrow.
#[cfg(any(not(windows), test))]
fn roots_from(get: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    const VARS: &[&str] = &[
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
        "APPDATA",
        "ProgramData",
    ];
    VARS.iter()
        .filter_map(|key| get(key))
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

fn roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Shell::{
            FOLDERID_LocalAppData, FOLDERID_ProgramData, FOLDERID_ProgramFiles,
            FOLDERID_ProgramFilesX86, FOLDERID_RoamingAppData,
        };
        [
            &FOLDERID_ProgramFiles,
            &FOLDERID_ProgramFilesX86,
            &FOLDERID_LocalAppData,
            &FOLDERID_RoamingAppData,
            &FOLDERID_ProgramData,
        ]
        .into_iter()
        .filter_map(crate::sysops::known_folder)
        .collect()
    }
    #[cfg(not(windows))]
    {
        roots_from(|key| std::env::var(key).ok())
    }
}

/// Every directory Remova itself writes under a snapshot root.
///
/// Once `%ProgramData%` and `%LOCALAPPDATA%` are snapshot roots, anything Remova
/// creates during the monitored window shows up in the end-of-session diff as a
/// path "added during the monitored install" — and the allow-list is armed from
/// that diff, so the monitor would offer to delete its own state. Remova's state
/// is never an install artifact.
///
/// This is deliberately a **list**, derived once, rather than a single literal at
/// the call site. The first version excluded only `%ProgramData%\Remova`, which
/// left `%ProgramData%\RemovaSealKeys` — the directory holding `seal.key`, the
/// HMAC key every RSEAL2 seal is MAC'd with — exposed. A first backup taken during
/// a monitored install creates that key, the monitor then reported it as an install
/// artifact with `Confirmed`/`Low` (i.e. preselected), and one click on Cleanup
/// destroyed the product's own undo safety net: every existing Safety Vault session
/// became unrestorable and `key::load(true)` returned `seal:key_lost`.
///
/// Adding a new Remova state location means adding it here; there is no second
/// place that has to remember to.
fn remova_state_roots() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |base: Option<String>, sub: &str| {
        if let Some(base) = base.filter(|v| !v.trim().is_empty()) {
            let candidate = PathBuf::from(base).join(sub);
            if !out.iter().any(|existing| existing == &candidate) {
                out.push(candidate);
            }
        }
    };
    #[cfg(windows)]
    let program_data =
        crate::sysops::known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramData)
            .map(|p| p.to_string_lossy().into_owned());
    #[cfg(windows)]
    let local_app_data =
        crate::sysops::known_folder(&windows::Win32::UI::Shell::FOLDERID_LocalAppData)
            .map(|p| p.to_string_lossy().into_owned());
    #[cfg(not(windows))]
    let program_data = std::env::var("PROGRAMDATA").ok();
    #[cfg(not(windows))]
    let local_app_data = std::env::var("LOCALAPPDATA").ok();
    // History, ignore list, Safety Vault sessions, monitor snapshot, seals.
    push(program_data.clone(), "Remova");
    // The privileged seal-key store: seal.key, initialized.v2, stage_<pid>_<nanos>.
    push(program_data, "RemovaSealKeys");
    // AI configuration (holds the user's cloud API key) and the icon cache.
    push(local_app_data, "Remova");
    out
}

fn is_noise_path(p: &str) -> bool {
    let low = p.to_lowercase().replace('/', "\\");
    low.contains("\\temp\\")
        || low.contains("\\tmp\\")
        || low.contains("\\cache\\")
        || low.contains("\\caches\\")
        || low.ends_with(".log")
        || low.ends_with(".tmp")
        || low.contains("\\logs\\")
        || low.contains("\\crashdumps\\")
        || low.contains("\\telemetry\\")
}

fn walk_names(
    root: &Path,
    out: &mut BTreeSet<String>,
    budget: &mut usize,
    depth: usize,
    truncated: &mut bool,
    exclude: &[PathBuf],
) {
    if crate::scan_task::cancelled() {
        *truncated = true;
        return;
    }
    if *budget == 0 || depth >= crate::constants::INSTALLMON_MAX_WALK_DEPTH {
        *truncated = true;
        return;
    }
    let rd = match std::fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                *truncated = true;
            }
            return;
        }
    };
    for e in rd {
        if !crate::scan_task::checkpoint("files", out.len()) {
            *truncated = true;
            return;
        }
        let e = match e {
            Ok(e) => e,
            Err(_) => {
                *truncated = true;
                continue;
            }
        };
        if *budget == 0 {
            *truncated = true;
            return;
        }
        let p = e.path();
        // Remova's own state is not an install artifact: reporting it would put the
        // monitor's own baseline into the diff it arms deletions from.
        //
        // The comparison is case-insensitive per component, because
        // `Path::starts_with` on Windows is a byte comparison that folds only the
        // drive letter — so `C:\ProgramData\remova` did not start with
        // `C:\ProgramData\Remova`, and neither did a path that merely differed in
        // drive-letter case. `read_dir` reports the casing stored on disk, so the
        // guard used to depend on the on-disk name of a directory Remova does not
        // own. Comparing `path_key` (already lowercase + backslash-normalized)
        // removes that dependency.
        if is_under_any(&p, exclude) {
            continue;
        }
        // Keep original casing for display/delete; NTFS compare is case-insensitive.
        // Non-UTF-8 never enters snapshots/allow-lists (fail-closed).
        let Some(rel) = crate::fsutil::path_utf8(&p) else {
            *truncated = true;
            continue;
        };
        *budget = budget.saturating_sub(1);
        out.insert(rel);
        // `is_symlink()` classifies junctions version-dependently and misses
        // non-symlink reparse dirs (cloud placeholders) — check the attribute.
        if p.is_dir() && !crate::fsutil::is_reparse_point(&p) {
            walk_names(&p, out, budget, depth + 1, truncated, exclude);
        }
    }
}

/// True when `path` is one of `roots` or sits anywhere underneath one.
///
/// Case-insensitive per component, matching how NTFS resolves names — a guard
/// that only holds for the exact on-disk casing is a guard an unrelated directory
/// name can switch off.
fn is_under_any(path: &Path, roots: &[PathBuf]) -> bool {
    let Some(key) = crate::fsutil::path_key(path) else {
        return false;
    };
    roots.iter().any(|root| {
        let Some(root_key) = crate::fsutil::path_key(root) else {
            return false;
        };
        key == root_key || key.starts_with(&format!("{root_key}\\"))
    })
}

fn take_fs_snapshot() -> FsSnapshot {
    let mut files = BTreeSet::new();
    let mut truncated = false;
    let mut budget = walk_budget();
    let exclude = remova_state_roots();
    for r in roots() {
        if crate::scan_task::cancelled() {
            truncated = true;
            break;
        }
        walk_names(&r, &mut files, &mut budget, 0, &mut truncated, &exclude);
    }
    FsSnapshot { files, truncated }
}

// tests shrink the walk budget so begin/end roundtrips stay fast.
#[cfg(test)]
fn walk_budget() -> usize {
    std::cmp::min(
        crate::constants::INSTALLMON_PATH_BUDGET,
        TEST_BUDGET.load(std::sync::atomic::Ordering::SeqCst),
    )
}

#[cfg(not(test))]
fn walk_budget() -> usize {
    crate::constants::INSTALLMON_PATH_BUDGET
}

#[cfg(test)]
static TEST_BUDGET: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(200);

fn reg_value_names() -> Result<BTreeSet<String>, String> {
    let mut set = BTreeSet::new();
    // align Run-family keys with manage::RUN_KEYS (HKLM32 Run, all
    // RunOnce views, Policies Explorer Run) so 32-bit / one-shot / policy
    // persistence shows up in the install-monitor diff.
    let mut keys: Vec<String> = vec![
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall".into(),
        r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall".into(),
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall".into(),
    ];
    keys.extend(crate::manage::run_key_paths());
    for k in keys {
        if !crate::scan_task::checkpoint("registry", set.len()) {
            return Err("scan:cancelled".into());
        }
        let (subkeys, values) = crate::regscan::snapshot_names(&k)?;
        for sub in subkeys {
            if crate::scan_task::cancelled() {
                return Err("scan:cancelled".into());
            }
            set.insert(format!("{k}\\{sub}").to_lowercase());
        }
        for v in values {
            if crate::scan_task::cancelled() {
                return Err("scan:cancelled".into());
            }
            // Value paths must use the pipeline-wide `key|value` shape — the
            // cleanup pipeline, backup and restore all split on `|`, so a
            // monitored Run value written as `key::value` can never be deleted.
            if !v.is_empty() {
                set.insert(format!("{k}|{v}").to_lowercase());
            }
        }
    }
    Ok(set)
}

fn unknown_registry_snapshot() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FullSnapshot {
    fs: FsSnapshot,
    reg: Vec<String>,
    #[serde(default = "unknown_registry_snapshot")]
    reg_degraded: bool,
}

/// one monitor session per process — begin/end are serialized so
/// concurrent calls (UI double-fire, tests) cannot interleave snapshot writes.
fn monitor_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

pub fn begin() -> Result<(), String> {
    let _guard = monitor_lock();
    if !crate::scan_task::checkpoint("preparing", 0) {
        return Err("scan:cancelled".into());
    }
    crate::scan_allow::remember(crate::scan_allow::AllowScope::Monitor, &Default::default());
    let reg = reg_value_names();
    let reg_degraded = reg.is_err();
    let snap = FullSnapshot {
        fs: take_fs_snapshot(),
        reg: reg.unwrap_or_default().into_iter().collect(),
        reg_degraded,
    };
    let p = monitor_state_path();
    let s = serde_json::to_string(&snap).map_err(|e| e.to_string())?;
    crate::scan_task::checkpoint("finalizing", snap.fs.files.len() + snap.reg.len());
    crate::scan_task::commit(|_| {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, s).map_err(|e| e.to_string())
    })
}

pub fn end() -> Result<MonitorEndResult, String> {
    let _guard = monitor_lock();
    if !crate::scan_task::checkpoint("preparing", 0) {
        return Err("scan:cancelled".into());
    }
    let p = monitor_state_path();
    let raw =
        std::fs::read_to_string(&p).map_err(|_| "no monitor snapshot; start first".to_string())?;
    let before: FullSnapshot = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let result = build_diff(before, take_fs_snapshot(), reg_value_names());
    let scanned = trusted_monitor_paths(&result);
    crate::scan_task::checkpoint(
        "finalizing",
        result.diff.added_files.len() + result.diff.added_reg_values.len(),
    );
    crate::scan_task::commit(|permit| {
        crate::scan_allow::commit_monitor_items(permit, &scanned, || {
            std::fs::remove_file(&p).map_err(|e| e.to_string())
        })?;
        Ok(result)
    })
}

fn build_diff(
    before: FullSnapshot,
    after: FsSnapshot,
    after_reg: Result<BTreeSet<String>, String>,
) -> MonitorEndResult {
    let registry_degraded = before.reg_degraded || after_reg.is_err();
    let after_reg = after_reg.unwrap_or_default();
    let added_files_all: Vec<String> = after.files.difference(&before.fs.files).cloned().collect();
    let files_truncated = added_files_all.len().saturating_sub(200);
    let added_files: Vec<String> = added_files_all.into_iter().take(200).collect();
    let before_reg: BTreeSet<String> = before.reg.into_iter().collect();
    let added_reg_all: Vec<String> = after_reg.difference(&before_reg).cloned().collect();
    let reg_truncated = added_reg_all.len().saturating_sub(100);
    let added_reg_values: Vec<String> = added_reg_all.into_iter().take(100).collect();
    let diff = MonitorDiff {
        added_files,
        added_reg_values,
        files_truncated,
        reg_truncated,
        walk_degraded: before.fs.truncated || after.truncated || registry_degraded,
    };
    // Allow-list is armed only from this server-computed diff (never from
    // client IPC) — and never from a truncated snapshot: entries missed by
    // the walk would resurface as ghost "added" paths and arm deletions the
    // user never actually saw in the diff.
    let items = diff_to_cleanup_items(&diff);
    MonitorEndResult { diff, items }
}

fn trusted_monitor_paths(result: &MonitorEndResult) -> std::collections::HashSet<String> {
    if result.diff.walk_degraded {
        return Default::default();
    }
    result.items.iter().map(|item| item.path.clone()).collect()
}

#[cfg(test)]
fn diff_and_arm(
    before: FullSnapshot,
    after: FsSnapshot,
    after_reg: Result<BTreeSet<String>, String>,
) -> MonitorEndResult {
    let result = build_diff(before, after, after_reg);
    crate::scan_allow::remember(
        crate::scan_allow::AllowScope::Monitor,
        &trusted_monitor_paths(&result),
    );
    result
}

/// Convert a monitor diff into CleanupItems for the existing cleanup pipeline.
/// Paths added during a monitored install are strong evidence (Confirmed/Low).
/// Noise (cache/temp/log) is demoted to Suspected/Medium so it is not auto-selected as "safe".
/// user-data / library red lines use the same classification as the
/// analyzer, never hardcoded `false`.
/// Roots where "this path changed while I was watching" is **not** evidence that
/// the monitored install created it.
///
/// `Program Files` and `%LOCALAPPDATA%` are per-machine/per-user install territory,
/// so a new path there during a monitored install is a reasonable candidate. But
/// `%APPDATA%` (Roaming) is where unrelated applications keep their live
/// configuration, and `%ProgramData%` is machine-wide shared by every product on
/// the box — OneDrive, Teams, Office, a browser, a driver updater, or another
/// user's non-elevated process all write there continuously.
///
/// Reporting those as `Confirmed`/`Low` preselected them, so one click on Cleanup
/// deleted an unrelated application's live configuration under a risk label that
/// actively said "low". They are still reported — the user asked what changed —
/// but as `Suspected`/`Medium`, which `defaultSelectable` never preselects.
const WEAK_EVIDENCE_ROOTS: &[&str] = &["APPDATA", "ProgramData"];

fn weak_evidence_roots() -> Vec<PathBuf> {
    WEAK_EVIDENCE_ROOTS
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Path identity for weak-root containment, using the same `path_key`
/// normalization as the delete gate so `C:\Users\A\AppData\Roaming\X`,
/// `c:/users/a/appdata/roaming/X` and `...\Roaming\X\` all match one root.
fn weak_root_keys() -> Vec<String> {
    weak_evidence_roots()
        .iter()
        .filter_map(|root| crate::fsutil::path_key(root))
        .collect()
}

fn is_weak_evidence_path(path: &str, weak_keys: &[String]) -> bool {
    if weak_keys.is_empty() {
        return false;
    }
    let Some(key) = crate::fsutil::path_key(std::path::Path::new(path)) else {
        return false;
    };
    weak_keys
        .iter()
        .any(|root| key == *root || key.starts_with(&format!("{root}\\")))
}

pub fn diff_to_cleanup_items(diff: &MonitorDiff) -> Vec<crate::scanner::CleanupItem> {
    use crate::scanner::{CleanupItem, Confidence, Evidence, ItemKind, RiskLevel};
    let mut items = Vec::new();
    let weak_keys = weak_root_keys();
    for f in &diff.added_files {
        let p = std::path::Path::new(f);
        let kind = if p.is_dir() {
            ItemKind::Dir
        } else {
            ItemKind::File
        };
        let noisy = is_noise_path(f);
        let user_data =
            crate::safety::is_user_data_path(f) || crate::safety::looks_like_sync_conflict(f);
        let user_library = !user_data && crate::safety::is_user_library_path(f);
        // A change under a shared root proves nothing about this install, so it is
        // never preselected and never labelled low-risk.
        let weak = !user_data && is_weak_evidence_path(f, &weak_keys);
        // Reported, but never preselected. `user_data` already blocks selection on
        // its own and keeps its own (higher) risk label, so it is excluded here.
        let demoted = (noisy || weak) && !user_data;
        items.push(CleanupItem {
            path: f.clone(),
            kind,
            score: if noisy {
                40
            } else if weak {
                50
            } else {
                70
            },
            confidence: if demoted {
                Confidence::Suspected
            } else {
                Confidence::Confirmed
            },
            risk: if user_data {
                RiskLevel::High
            } else if noisy || weak {
                RiskLevel::Medium
            } else {
                RiskLevel::Low
            },
            reason: if noisy {
                "Install monitor: cache/log-like path".into()
            } else if weak {
                "Install monitor: changed in a shared directory".into()
            } else {
                "Install monitor: new path".into()
            },
            evidence: vec![Evidence {
                code: "install_monitor".into(),
                label: if noisy {
                    "Added during install (likely cache/temp)".into()
                } else if weak {
                    "Added during monitored install (shared directory, unproven)".into()
                } else {
                    "Added during monitored install".into()
                },
                weight: if noisy {
                    40
                } else if weak {
                    50
                } else {
                    70
                },
                detail: String::new(),
            }],
            shared: false,
            service_bound: false,
            user_data,
            user_library,
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
    for r in &diff.added_reg_values {
        items.push(CleanupItem {
            path: r.clone(),
            kind: crate::scanner::ItemKind::Registry,
            score: 70,
            confidence: Confidence::Confirmed,
            risk: RiskLevel::Low,
            reason: "Install monitor: new registry entry".into(),
            evidence: vec![Evidence {
                code: "install_monitor".into(),
                label: "Added during monitored install".into(),
                weight: 70,
                detail: String::new(),
            }],
            shared: false,
            service_bound: false,
            user_data: false,
            user_library: false,
            size_kb: None,
            size_capped: false,
            bucket: None,
        });
    }
    crate::scanner::fill_item_sizes(&mut items);
    crate::scanner::fill_item_buckets(&mut items, "");
    // Pure conversion only. Allow-list arming happens in `end()` on the server-computed diff
    // so a forged client `MonitorDiff` cannot unlock arbitrary deletes.
    items
}

#[cfg(test)]
mod tests {
    #[test]
    fn cancelled_session_capture_preserves_the_existing_baseline() {
        let _seq = TEST_SEQ.lock().unwrap_or_else(|e| e.into_inner());
        let _allow = crate::scan_allow::test_lock();
        let state =
            std::env::temp_dir().join(format!("remova-monitor-cancel-{}.json", std::process::id()));
        let raw = serde_json::to_string(&super::FullSnapshot {
            fs: Default::default(),
            reg: vec![],
            reg_degraded: false,
        })
        .unwrap();
        std::fs::write(&state, &raw).unwrap();
        set_test_state_path(Some(state.clone()));
        for capture in [super::begin as fn() -> Result<(), String>, || {
            super::end().map(|_| ())
        }] {
            let id = crate::scan_task::begin().unwrap();
            let result = crate::scan_task::run(id, || {
                crate::scan_task::cancel(id).unwrap();
                assert_eq!(capture(), Err("scan:cancelled".into()));
            });
            assert_eq!(result, Err("scan:cancelled".into()));
            assert_eq!(std::fs::read_to_string(&state).unwrap(), raw);
        }
        set_test_state_path(None);
        std::fs::remove_file(state).unwrap();
    }
    #[test]
    fn cancelled_snapshot_walk_never_reads_or_consumes_budget() {
        let id = crate::scan_task::begin().unwrap();
        let mut budget = 20;
        let mut names = std::collections::BTreeSet::new();
        let mut truncated = false;
        let result = crate::scan_task::run(id, || {
            crate::scan_task::cancel(id).unwrap();
            super::walk_names(
                std::path::Path::new("cancelled-monitor-unread"),
                &mut names,
                &mut budget,
                0,
                &mut truncated,
                &[],
            );
            assert!(super::reg_value_names().is_err());
        });
        assert_eq!(result, Err("scan:cancelled".into()));
        assert_eq!(budget, 20);
        assert!(names.is_empty() && truncated);
    }
    #[test]
    fn incomplete_registry_snapshots_never_arm_existing_items() {
        let _allow = crate::scan_allow::test_lock();
        let path = r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run|Vendor".to_lowercase();
        for failed_before in [true, false] {
            crate::scan_allow::remember(
                crate::scan_allow::AllowScope::Monitor,
                &[path.clone()].into(),
            );
            let before = super::FullSnapshot {
                fs: super::FsSnapshot::default(),
                reg: vec![],
                reg_degraded: failed_before,
            };
            let after_reg = if failed_before {
                Ok([path.clone()].into())
            } else {
                Err("denied".into())
            };
            let result = super::diff_and_arm(before, super::FsSnapshot::default(), after_reg);
            assert!(result.diff.walk_degraded);
            assert!(!crate::scan_allow::was_recent(
                crate::scan_allow::AllowScope::Monitor,
                &path
            ));
            for item in result.items {
                assert!(!crate::scan_allow::was_recent(
                    crate::scan_allow::AllowScope::Monitor,
                    &item.path
                ));
            }
        }
        let legacy: super::FullSnapshot =
            serde_json::from_str(r#"{"fs":{"files":[]},"reg":[]}"#).unwrap();
        assert!(legacy.reg_degraded, "unknown completeness must fail closed");
    }
    use super::*;

    /// Serializes tests that swap the process-global state path / walk budget —
    /// begin()/end() take monitor_lock() themselves, so those must never be
    /// held while acquiring this one.
    static TEST_SEQ: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn invalid_directory_snapshot_is_degraded() {
        let tmp = std::env::temp_dir().join(format!("remova_mon_file_{}", std::process::id()));
        std::fs::write(&tmp, b"not a directory").unwrap();
        let mut degraded = false;
        walk_names(&tmp, &mut BTreeSet::new(), &mut 100, 0, &mut degraded, &[]);
        assert!(degraded);
        std::fs::remove_file(tmp).unwrap();
    }

    /// `%APPDATA%` and `%ProgramData%` are install roots: an installer that
    /// writes only there used to yield an empty diff while the UI said the scan
    /// completed. The rest of the product already walks both.
    #[test]
    fn snapshot_roots_cover_appdata_and_programdata() {
        let env = [
            ("ProgramFiles", "C:/Program Files"),
            ("ProgramFiles(x86)", "C:/Program Files (x86)"),
            ("LOCALAPPDATA", "C:/Users/x/AppData/Local"),
            ("APPDATA", "C:/Users/x/AppData/Roaming"),
            ("ProgramData", "C:/ProgramData"),
        ];
        let roots = super::roots_from(|key| {
            env.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        });
        for want in ["AppData/Roaming", "ProgramData"] {
            assert!(
                roots
                    .iter()
                    .any(|r| r.to_string_lossy().replace('\\', "/").ends_with(want)),
                "{want} must be a snapshot root, got {roots:?}"
            );
        }
        // A relocated root is read from the environment, never rebuilt.
        let relocated = super::roots_from(|key| match key {
            "APPDATA" => Some("D:/Profiles/x/AppData/Roaming".to_string()),
            "ProgramData" => Some("D:/Profiles/ProgramData".to_string()),
            _ => None,
        });
        assert_eq!(relocated.len(), 2, "{relocated:?}");
        assert!(relocated.iter().any(|r| r.ends_with("Roaming")));
        assert!(relocated.iter().any(|r| r.ends_with("ProgramData")));
    }

    /// Remova writes three state directories under snapshot roots, and only one of
    /// them used to be excluded. `RemovaSealKeys` holds `seal.key` — the HMAC key
    /// every RSEAL2 seal is MAC'd with — which a first Safety Vault backup taken
    /// during a monitored install creates. It then appeared in the end-of-session
    /// diff as an install artifact, was rated Confirmed/Low (preselected), and one
    /// click on Cleanup made every existing session unrestorable.
    ///
    /// The exclusion is also case-insensitive per component: `Path::starts_with`
    /// folds only the drive letter on Windows, so a state directory that happened
    /// to be created as `remova` rather than `Remova` switched the guard off.
    #[test]
    fn removas_own_state_is_never_a_snapshot_candidate() {
        let root = crate::test_support::TempDir::new("mon-own");
        // Three state roots, one of them deliberately mis-cased on disk.
        let state = root.join("Remova");
        let seal_keys = root.join("RemovaSealKeys");
        let mis_cased = root.join("remova");
        for dir in [&state, &seal_keys, &mis_cased] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(state.join("monitor_snapshot.json"), b"{}").unwrap();
        std::fs::write(seal_keys.join("seal.key"), b"k".repeat(32)).unwrap();
        std::fs::write(seal_keys.join("initialized.v2"), b"1").unwrap();
        std::fs::write(mis_cased.join("ignore.json"), b"{}").unwrap();
        // A real install target next to them must still be snapshotted.
        let vendor = root.join("Vendor");
        std::fs::create_dir_all(&vendor).unwrap();
        std::fs::write(vendor.join("app.bin"), b"payload").unwrap();

        let mut names = BTreeSet::new();
        let mut budget = 100;
        let mut truncated = false;
        walk_names(
            &root,
            &mut names,
            &mut budget,
            0,
            &mut truncated,
            &[state, seal_keys, mis_cased],
        );

        assert!(
            names.iter().any(|n| n.ends_with("app.bin")),
            "a real install target must still be snapshotted: {names:?}"
        );
        for n in &names {
            let lower = n.replace('/', "\\").to_lowercase();
            for marker in ["\\remova\\", "\\removasealkeys\\"] {
                assert!(
                    !lower.contains(marker),
                    "Remova's own state must never enter the snapshot ({marker}): {n}"
                );
            }
        }
    }

    /// The exclusion must be case-insensitive per component, so a state directory
    /// created with different casing cannot switch the guard off.
    #[test]
    fn remova_state_exclusion_ignores_casing() {
        let root = crate::test_support::TempDir::new("mon-case");
        let excluded = root.join("RemovaSealKeys");
        std::fs::create_dir_all(excluded.join("stage_1_2")).unwrap();
        std::fs::write(excluded.join("seal.key"), b"k".repeat(32)).unwrap();
        let lower_cased = std::path::PathBuf::from(excluded.to_string_lossy().to_lowercase());
        for candidate in [
            excluded.clone(),
            excluded.join("seal.key"),
            excluded.join("stage_1_2"),
            lower_cased,
        ] {
            assert!(
                super::is_under_any(&candidate, std::slice::from_ref(&excluded)),
                "must be excluded regardless of casing: {candidate:?}"
            );
        }
        // A sibling that merely shares a prefix is NOT excluded.
        let sibling = root.join("RemovaSealKeysExtra");
        std::fs::create_dir_all(&sibling).unwrap();
        assert!(
            !super::is_under_any(&sibling, std::slice::from_ref(&excluded)),
            "a same-prefix sibling must not be excluded"
        );
    }

    #[test]
    fn snapshot_roundtrip_shape() {
        let _seq = TEST_SEQ.lock().unwrap_or_else(|e| e.into_inner());
        let _allow = crate::scan_allow::test_lock();
        // isolated state path — tests never touch real PROGRAMDATA.
        let tmp = std::env::temp_dir().join(format!("remova_mon_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let state = tmp.join("monitor_snapshot.json");
        set_test_state_path(Some(state.clone()));
        // end() without begin() must error (no snapshot at the injected path).
        let e2 = super::end();
        assert!(e2.is_err());
        // begin() writes the snapshot at the injected path; end() consumes it.
        super::begin().unwrap();
        assert!(state.exists());
        super::end().unwrap();
        assert!(!state.exists(), "end() consumes the snapshot");
        set_test_state_path(None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Monitored registry values must flow through the pipeline's value-path
    /// contract: diff entry → CleanupItem → `split_value_path` → (key, value).
    #[test]
    fn diff_value_paths_split_into_key_and_value() {
        let diff = MonitorDiff {
            added_files: vec![],
            added_reg_values: vec![
                r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run|VendorTray".to_lowercase(),
                r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce|Setup".to_lowercase(),
            ],
            files_truncated: 0,
            reg_truncated: 0,
            walk_degraded: false,
        };
        let items = diff_to_cleanup_items(&diff);
        assert_eq!(items.len(), 2);
        for it in &items {
            let (k, v) = crate::regops::split_value_path(&it.path)
                .expect("monitored value must use key|value");
            assert!(
                !k.is_empty() && !v.is_empty(),
                "key and value must be non-empty: {it:?}"
            );
        }
        // Empty value names never enter the snapshot (a trailing `|` would be
        // rejected by the pipeline as an attempt to delete the whole key).
        assert!(crate::regops::split_value_path(r"HKCU\Software\Run|").is_none());
    }

    /// Client-supplied diff must NOT arm the Monitor allow-list.
    #[test]
    fn diff_to_items_does_not_arm_allow_list() {
        let forged = MonitorDiff {
            added_files: vec![r"C:\Windows\System32\evil.dll".into()],
            added_reg_values: vec![],
            files_truncated: 0,
            reg_truncated: 0,
            walk_degraded: false,
        };
        let items = diff_to_cleanup_items(&forged);
        assert_eq!(items.len(), 1);
        assert!(
            !crate::scan_allow::was_recent(
                crate::scan_allow::AllowScope::Monitor,
                r"C:\Windows\System32\evil.dll"
            ),
            "pure conversion must not remember paths"
        );
    }

    /// monitor items carry the analyzer's user-data / library red lines.
    #[test]
    fn diff_to_cleanup_items_red_lines() {
        let diff = MonitorDiff {
            added_files: vec![
                r"C:\Users\testuser\Documents\SaveGame Editor\saves.db".into(),
                r"C:\Users\testuser\Documents".into(),
                r"C:\Program Files\Vendor\App\new.dll".into(),
            ],
            added_reg_values: vec![],
            files_truncated: 0,
            reg_truncated: 0,
            walk_degraded: false,
        };
        let items = diff_to_cleanup_items(&diff);
        let under_docs = items.iter().find(|i| i.path.ends_with("saves.db")).unwrap();
        assert!(under_docs.user_library, "under Documents → user_library");
        assert!(!under_docs.user_data);
        let docs_root = items
            .iter()
            .find(|i| i.path.ends_with("Documents"))
            .unwrap();
        assert!(docs_root.user_data, "Documents root → user_data");
        assert_eq!(docs_root.risk, crate::scanner::RiskLevel::High);
        assert!(!under_docs.user_data && !docs_root.user_library);
        let pf = items.iter().find(|i| i.path.ends_with("new.dll")).unwrap();
        assert!(!pf.user_data && !pf.user_library);
    }

    /// A change under `%APPDATA%` or `%ProgramData%` proves nothing about the
    /// monitored install — those roots are shared with every other product on the
    /// machine. They must never be preselected and never labelled low-risk, or one
    /// click on Cleanup deletes an unrelated application's live configuration.
    #[test]
    fn shared_root_additions_are_never_preselected() {
        let _guard = crate::test_support::EnvLock::new();
        let root = crate::test_support::TempDir::new("mon-weak");
        // Point the two weak-evidence roots at the fixture so the test does not
        // depend on the machine's real `%APPDATA%`/`%ProgramData%`.
        let roaming = root.subdir("Roaming");
        let machine = root.subdir("Machine");
        let _roaming_var = crate::test_support::EnvVar::set("APPDATA", &roaming);
        let _machine_var = crate::test_support::EnvVar::set("ProgramData", &machine);
        let weak_keys = super::weak_root_keys();
        assert_eq!(weak_keys.len(), 2);
        for probe in [
            roaming.join("Teams\\cache").to_string_lossy().into_owned(),
            // Same path spelled with forward slashes, a trailing separator and
            // different case must resolve to the same root: `path_key` is the
            // delete gate's path identity, so containment uses it too.
            roaming.join("teams/").to_string_lossy().into_owned(),
            machine
                .join("SomeDriver\\config")
                .to_string_lossy()
                .into_owned(),
        ] {
            assert!(
                super::is_weak_evidence_path(&probe, &weak_keys),
                "{probe} should be weak evidence"
            );
        }
        // A sibling of the fixture root is outside both weak roots.
        assert!(!super::is_weak_evidence_path(
            &root.subdir("Elsewhere\\app.exe").to_string_lossy(),
            &weak_keys
        ));
        assert!(!super::is_weak_evidence_path("", &weak_keys));
        assert!(!super::is_weak_evidence_path("   ", &weak_keys));

        let diff = MonitorDiff {
            added_files: vec![
                roaming
                    .join("Teams\\cache.db")
                    .to_string_lossy()
                    .into_owned(),
                machine
                    .join("SomeDriver\\config.ini")
                    .to_string_lossy()
                    .into_owned(),
                // A synthetic install-root path: the fixture itself lives under
                // `%TEMP%`, which `is_noise_path` would (correctly) demote, so a
                // fixture file cannot stand in for a real install location.
                r"C:\Program Files\Vendor\App\app.exe".into(),
            ],
            added_reg_values: vec![],
            files_truncated: 0,
            reg_truncated: 0,
            walk_degraded: false,
        };
        let items = diff_to_cleanup_items(&diff);
        for path in ["cache.db", "config.ini"] {
            let item = items.iter().find(|i| i.path.ends_with(path)).unwrap();
            assert_eq!(
                item.confidence,
                crate::scanner::Confidence::Suspected,
                "{path} lives in a shared root and must not be Confirmed"
            );
            assert_eq!(
                item.risk,
                crate::scanner::RiskLevel::Medium,
                "{path} must not be labelled low-risk"
            );
            // The frontend preselects on `confirmed && !high && !shared &&
            // !user_data`; the server-side gate is the authority, so assert both.
            assert!(
                item.confidence != crate::scanner::Confidence::Confirmed
                    && item.risk != crate::scanner::RiskLevel::Low,
                "{path} must not satisfy the frontend's preselect predicate"
            );
        }
        // A path in a real install root is still strong evidence.
        let vendor = items.iter().find(|i| i.path.ends_with("app.exe")).unwrap();
        assert_eq!(vendor.confidence, crate::scanner::Confidence::Confirmed);
        assert_eq!(vendor.risk, crate::scanner::RiskLevel::Low);
    }

    /// Convert path only; allow-list is armed by `end()` on the server diff.
    #[test]
    fn diff_to_cleanup_items_shapes() {
        let _allow = crate::scan_allow::test_lock();
        let diff = MonitorDiff {
            added_files: vec![
                r"C:\Program Files\Vendor\App\new.dll".into(),
                r"C:\Program Files\Vendor\App\cache.tmp".into(),
            ],
            added_reg_values: vec![
                r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{NEW}".into(),
            ],
            files_truncated: 0,
            reg_truncated: 0,
            walk_degraded: false,
        };
        let items = diff_to_cleanup_items(&diff);
        assert_eq!(items.len(), 3);
        // Noise path is still an item (kept-list explanation) but marked suspected.
        let noise = items
            .iter()
            .find(|i| i.path.ends_with("cache.tmp"))
            .unwrap();
        assert_eq!(noise.confidence, crate::scanner::Confidence::Suspected);
        // Gate: associated-less Monitor items still pass the scoped allow-list path.
        let ignore = crate::ignore::IgnoreList::default();
        let app = crate::apps::InstalledApp {
            name: "Monitor".into(),
            version: String::new(),
            publisher: String::new(),
            install_location: String::new(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            source: "monitor".into(),
            registry_key: String::new(),
            estimated_size_kb: 0,
            install_date: String::new(),
            display_icon: String::new(),
        };
        // Without allow-list arming, gate must skip.
        for it in &items {
            let d = crate::policy::gate_cleanup_item(
                Some(&app),
                it,
                crate::policy::CleanupSource::Monitor,
                &ignore,
            );
            assert!(
                matches!(d, crate::policy::GateDecision::Skip(_)),
                "unarmed monitor item must skip: {it:?} → {d:?}"
            );
        }
        // Simulate end() arming, then gate passes.
        let mut scanned = std::collections::HashSet::new();
        for it in &items {
            scanned.insert(it.path.clone());
        }
        crate::scan_allow::remember(crate::scan_allow::AllowScope::Monitor, &scanned);
        for it in &items {
            let d = crate::policy::gate_cleanup_item(
                Some(&app),
                it,
                crate::policy::CleanupSource::Monitor,
                &ignore,
            );
            assert!(
                matches!(d, crate::policy::GateDecision::Allow),
                "armed monitor item must pass gate: {it:?} → {d:?}"
            );
        }
    }

    /// Junctions are reparse points, not symlinks — `is_symlink` misses them
    /// (the same trap dirsize.rs hit). A planted junction must never be
    /// followed into its target, or a cycle can recurse to stack overflow.
    #[cfg(windows)]
    #[test]
    fn walk_names_never_follows_junctions() {
        let base = std::env::temp_dir().join(format!(
            "remova_walk_junction_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let a = base.join("A");
        let b = base.join("B");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("own.txt"), b"x").unwrap();
        std::fs::write(b.join("target.txt"), b"y").unwrap();
        // Junctions need no privilege, unlike dir symlinks.
        let link = a.join("link");
        let mk = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&b)
            .output()
            .unwrap();
        assert!(
            mk.status.success(),
            "mklink /J failed: {}",
            String::from_utf8_lossy(&mk.stderr)
        );
        let mut files = BTreeSet::new();
        let mut budget = 100usize;
        let mut truncated = false;
        walk_names(&a, &mut files, &mut budget, 0, &mut truncated, &[]);
        assert!(
            files.contains(&a.join("own.txt").to_string_lossy().to_string()),
            "own file must be walked"
        );
        let via_link = a
            .join("link")
            .join("target.txt")
            .to_string_lossy()
            .to_string();
        assert!(
            !files.contains(&via_link),
            "junction target contents must never enter the snapshot: {via_link}"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Depth cap is defense-in-depth: NTFS dirs cannot cycle without a reparse
    /// point, but the walk must stay bounded regardless of std version behavior.
    #[test]
    fn walk_names_caps_depth() {
        let base = std::env::temp_dir().join(format!(
            "remova_walk_depth_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut deep = base.clone();
        for i in 0..80 {
            deep = deep.join(format!("d{i}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        let mut files = BTreeSet::new();
        let mut budget = 10_000usize;
        let mut truncated = false;
        walk_names(&base, &mut files, &mut budget, 0, &mut truncated, &[]);
        assert!(truncated, "hitting the depth cap must flag the snapshot");
        let cap = crate::constants::INSTALLMON_MAX_WALK_DEPTH;
        assert!(
            files
                .iter()
                .any(|p| p.ends_with(&format!(r"\d{}", cap - 1))),
            "walk should reach just under the cap boundary (d{})",
            cap - 1
        );
        assert!(
            !files.iter().any(|p| p.contains(&format!(r"\d{cap}"))),
            "walk must stop at INSTALLMON_MAX_WALK_DEPTH"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A budget-truncated snapshot cannot distinguish "existed at begin"
    /// from "added" — end() must degrade (return the diff unarmed), never
    /// arm deletions the user did not see.
    #[test]
    fn budget_exhausted_end_returns_diff_without_arming() {
        let _seq = TEST_SEQ.lock().unwrap_or_else(|e| e.into_inner());
        let _allow = crate::scan_allow::test_lock();
        let tmp = std::env::temp_dir().join(format!("remova_mon_budget_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let state = tmp.join("monitor_snapshot.json");
        set_test_state_path(Some(state.clone()));
        let stale = "stale_monitor_authorization".to_string();
        crate::scan_allow::remember(
            crate::scan_allow::AllowScope::Monitor,
            &[stale.clone()].into(),
        );
        TEST_BUDGET.store(3, std::sync::atomic::Ordering::SeqCst);
        super::begin().unwrap();
        assert!(!crate::scan_allow::was_recent(
            crate::scan_allow::AllowScope::Monitor,
            &stale
        ));
        crate::scan_allow::remember(
            crate::scan_allow::AllowScope::Monitor,
            &[stale.clone()].into(),
        );
        let res = super::end();
        TEST_BUDGET.store(200, std::sync::atomic::Ordering::SeqCst);
        set_test_state_path(None);
        let _ = std::fs::remove_dir_all(&tmp);
        let res = res.expect("end must succeed even on a truncated walk");
        // The diff itself reports the degradation so the UI can say the scan
        // is incomplete instead of presenting a possibly-empty diff as truth.
        assert!(res.diff.walk_degraded, "truncated walk must flag the diff");
        assert!(!crate::scan_allow::was_recent(
            crate::scan_allow::AllowScope::Monitor,
            &stale
        ));
        // Whatever items the diff produced, none may be armed.
        for it in &res.items {
            assert!(
                !crate::scan_allow::was_recent(crate::scan_allow::AllowScope::Monitor, &it.path),
                "truncated walk must not arm: {}",
                it.path
            );
        }
    }
}
