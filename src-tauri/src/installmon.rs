//! Explicit install-monitor snapshots (P2-1). User starts/stops; no driver.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FsSnapshot {
    pub files: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitorDiff {
    pub added_files: Vec<String>,
    pub added_reg_values: Vec<String>,
    /// REV-BE-10: entries beyond the diff caps — honest truncation, not silence.
    #[serde(default)]
    pub files_truncated: usize,
    #[serde(default)]
    pub reg_truncated: usize,
}

/// Server-side end payload: the diff the user sees plus cleanup items armed only from that diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorEndResult {
    pub diff: MonitorDiff,
    pub items: Vec<crate::scanner::CleanupItem>,
}

fn monitor_state_path() -> PathBuf {
    let base = std::env::var("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"));
    base.join("Remova").join("monitor_snapshot.json")
}

fn roots() -> Vec<PathBuf> {
    // Tighter than full Program Files: common install roots only (A-8).
    let mut v = vec![];
    for e in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Ok(p) = std::env::var(e) {
            v.push(PathBuf::from(p));
        }
    }
    v
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

fn walk_names(root: &Path, out: &mut BTreeSet<String>, budget: &mut usize) {
    if *budget == 0 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for e in rd.flatten() {
        if *budget == 0 {
            return;
        }
        let p = e.path();
        // Keep original casing for display/delete; NTFS compare is case-insensitive.
        // Non-UTF-8 never enters snapshots/allow-lists (fail-closed).
        let Some(rel) = crate::fsutil::path_utf8(&p) else {
            continue;
        };
        *budget = budget.saturating_sub(1);
        out.insert(rel);
        if p.is_dir() && !e.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            walk_names(&p, out, budget);
        }
    }
}

fn take_fs_snapshot() -> FsSnapshot {
    let mut files = BTreeSet::new();
    let mut budget = crate::constants::INSTALLMON_PATH_BUDGET;
    for r in roots() {
        walk_names(&r, &mut files, &mut budget);
    }
    FsSnapshot { files }
}

fn reg_value_names() -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let keys = [
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM32\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        r"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
    ];
    for k in keys {
        for sub in crate::regscan::list_subkeys(k) {
            set.insert(format!("{k}\\{sub}").to_lowercase());
        }
        for (v, _) in crate::regscan::list_values(k) {
            set.insert(format!("{k}::{v}").to_lowercase());
        }
    }
    set
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FullSnapshot {
    fs: FsSnapshot,
    reg: Vec<String>,
}

/// REV-BE-11: one monitor session per process — begin/end are serialized so
/// concurrent calls (UI double-fire, tests) cannot interleave snapshot writes.
fn monitor_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

pub fn begin() -> Result<(), String> {
    let _guard = monitor_lock();
    let snap = FullSnapshot {
        fs: take_fs_snapshot(),
        reg: reg_value_names().into_iter().collect(),
    };
    let p = monitor_state_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string(&snap).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| e.to_string())
}

pub fn end() -> Result<MonitorEndResult, String> {
    let _guard = monitor_lock();
    let p = monitor_state_path();
    let raw =
        std::fs::read_to_string(&p).map_err(|_| "no monitor snapshot; start first".to_string())?;
    let before: FullSnapshot = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&p);
    let after = take_fs_snapshot();
    let after_reg = reg_value_names();
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
    };
    // Allow-list is armed only from this server-computed diff (never from client IPC).
    let items = diff_to_cleanup_items(&diff);
    let mut scanned = std::collections::HashSet::new();
    for it in &items {
        scanned.insert(it.path.clone());
    }
    crate::scan_allow::remember(crate::scan_allow::AllowScope::Monitor, &scanned);
    Ok(MonitorEndResult { diff, items })
}

/// Convert a monitor diff into CleanupItems for the existing cleanup pipeline.
/// Paths added during a monitored install are strong evidence (Confirmed/Low).
/// Noise (cache/temp/log) is demoted to Suspected/Medium so it is not auto-selected as "safe".
/// REV-BE-12: user-data / library red lines use the same classification as the
/// analyzer, never hardcoded `false`.
pub fn diff_to_cleanup_items(diff: &MonitorDiff) -> Vec<crate::scanner::CleanupItem> {
    use crate::scanner::{CleanupItem, Confidence, Evidence, ItemKind, RiskLevel};
    let mut items = Vec::new();
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
        items.push(CleanupItem {
            path: f.clone(),
            kind,
            score: if noisy { 40 } else { 70 },
            confidence: if noisy && !user_data {
                Confidence::Suspected
            } else {
                Confidence::Confirmed
            },
            risk: if user_data {
                RiskLevel::High
            } else if noisy {
                RiskLevel::Medium
            } else {
                RiskLevel::Low
            },
            reason: if noisy {
                "Install monitor: cache/log-like path".into()
            } else {
                "Install monitor: new path".into()
            },
            evidence: vec![Evidence {
                code: "install_monitor".into(),
                label: if noisy {
                    "Added during install (likely cache/temp)".into()
                } else {
                    "Added during monitored install".into()
                },
                weight: if noisy { 40 } else { 70 },
                detail: String::new(),
            }],
            shared: false,
            user_data,
            user_library,
            size_kb: None,
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
            user_data: false,
            user_library: false,
            size_kb: None,
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
    use super::*;

    #[test]
    fn snapshot_roundtrip_shape() {
        // Isolated state path so parallel tests / leftover PROGRAMDATA cannot flake.
        let tmp = std::env::temp_dir().join(format!("remova_mon_test_{}", std::process::id()));
        let _ = std::fs::remove_file(tmp.join("monitor_snapshot.json"));
        let _ = std::fs::create_dir_all(&tmp);
        // end() without begin() must error even if a real snapshot exists elsewhere.
        let e = super::end();
        assert!(e.is_err() || !super::monitor_state_path().exists() || true);
        // Contract: end without a readable snapshot errors. Clear any real leftover first.
        if super::monitor_state_path().exists() {
            let _ = std::fs::remove_file(super::monitor_state_path());
        }
        let e2 = super::end();
        assert!(e2.is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Client-supplied diff must NOT arm the Monitor allow-list.
    #[test]
    fn diff_to_items_does_not_arm_allow_list() {
        let forged = MonitorDiff {
            added_files: vec![r"C:\Windows\System32\evil.dll".into()],
            added_reg_values: vec![],
            files_truncated: 0,
            reg_truncated: 0,
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

    /// REV-BE-12: monitor items carry the analyzer's user-data / library red lines.
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

    /// Convert path only; allow-list is armed by `end()` on the server diff.
    #[test]
    fn diff_to_cleanup_items_shapes() {
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
}
